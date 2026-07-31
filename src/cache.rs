//! 文章列表快取 — 透過 notify 檔案監控 + 定時掃描，避免每次請求都遍歷檔案系統。

use crate::articles::{scan_articles, Article};
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::RwLock;

/// 收到檔案變動事件後的靜默期（debounce）：期間內合併事件，避免頻繁全量重掃
const DEBOUNCE_DURATION: Duration = Duration::from_secs(2);
/// 定時掃描間隔：兜底 notify 漏報（如 Docker overlay 掛載、部分平台限制）
pub const PERIODIC_SCAN_INTERVAL: Duration = Duration::from_secs(30);

/// refresh 回呼型別：每次掃描後以最新文章列表呼叫（供搜尋索引等下游重建）
type RefreshHook = Box<dyn Fn(Vec<Article>) + Send + Sync>;

/// 文章列表快取
pub struct ArticleCache {
    articles: RwLock<Vec<Article>>,
    article_dir: PathBuf,
    /// 每次 refresh 後呼叫的回呼（傳入最新文章列表），供搜尋索引等
    /// 依賴同一變更訊號的下游同步重建。同步呼叫，回呼內可自行 spawn。
    refresh_hook: Mutex<Option<RefreshHook>>,
}

impl ArticleCache {
    /// 建立快取並執行初始全量掃描
    pub fn new(article_dir: PathBuf) -> Self {
        let articles = scan_articles(&article_dir);
        Self {
            articles: RwLock::new(articles),
            article_dir,
            refresh_hook: Mutex::new(None),
        }
    }

    /// 設定 refresh 回呼：每次掃描（notify debounce / 定時）完成後，以最新
    /// 文章列表呼叫一次。僅能設定一次；設定後不可移除。
    pub fn set_refresh_hook<F>(&self, hook: F)
    where
        F: Fn(Vec<Article>) + Send + Sync + 'static,
    {
        *self.refresh_hook.lock().unwrap_or_else(|e| e.into_inner()) = Some(Box::new(hook));
    }

    /// 回傳文章列表（快取副本，不觸碰檔案系統）
    pub async fn get_articles(&self) -> Vec<Article> {
        self.articles.read().await.clone()
    }

    /// 全量重掃並原子替換快取，接著觸發 refresh 回呼
    pub async fn refresh(&self) {
        let articles = scan_articles(&self.article_dir);
        *self.articles.write().await = articles.clone();
        if let Some(hook) = self.refresh_hook.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
            hook(articles);
        }
    }

    /// 啟動 notify watcher（背景任務）：偵測 article/ 下 `.md` 檔案的
    /// 建立 / 修改 / 移除事件，debounce 靜默期後觸發全量重掃。
    pub fn spawn_watcher(self: &Arc<Self>) {
        let cache = Arc::clone(self);
        tokio::spawn(async move {
            let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<()>();
            let tx_for_handler = tx.clone();

            // notify 回呼為同步上下文，透過無界通道橋接到 async
            let handler = move |res: notify::Result<Event>| {
                if let Ok(event) = res {
                    if is_relevant_event(&event) {
                        let _ = tx_for_handler.send(());
                    }
                }
            };

            let mut watcher = match RecommendedWatcher::new(handler, notify::Config::default()) {
                Ok(w) => w,
                Err(err) => {
                    tracing::warn!("⚠️  無法啟動檔案監控（{err}），僅依賴定時掃描");
                    return;
                }
            };

            if let Err(err) = watcher.watch(&cache.article_dir, RecursiveMode::Recursive) {
                tracing::warn!("⚠️  無法監控文章目錄（{err}），僅依賴定時掃描");
                return;
            }

            tracing::info!("👀 檔案監控已啟動: {}", cache.article_dir.display());

            // debounce：收到首個事件後，等待靜默期；期間有新事件則重置計時
            loop {
                if rx.recv().await.is_none() {
                    break;
                }
                loop {
                    tokio::select! {
                        _ = rx.recv() => { /* 有後續事件，重置靜默期 */ }
                        _ = tokio::time::sleep(DEBOUNCE_DURATION) => break,
                    }
                }
                cache.refresh().await;
            }
        });
    }

    /// 啟動定時全量掃描（背景任務）：兜底 notify 漏報
    pub fn spawn_periodic_scan(self: &Arc<Self>, interval: Duration) {
        let cache = Arc::clone(self);
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(interval);
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                ticker.tick().await;
                cache.refresh().await;
            }
        });
    }
}

/// 判斷事件是否與文章列表相關：`.md` 檔案的建立 / 修改 / 移除
fn is_relevant_event(event: &Event) -> bool {
    let kind_relevant = matches!(
        event.kind,
        EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_)
    );
    if !kind_relevant {
        return false;
    }
    event
        .paths
        .iter()
        .any(|p| p.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("md")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use notify::event::{
        AccessKind, AccessMode, CreateKind, DataChange, ModifyKind, RemoveKind,
    };
    use std::fs;
    use std::path::Path;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_article_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "daily_knowledge_cache_test_{tag}_{}_{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[tokio::test]
    async fn test_initial_scan_and_get() {
        let dir = temp_article_dir("init");
        fs::write(dir.join("a.md"), "# a").unwrap();

        let cache = ArticleCache::new(dir.clone());
        let articles = cache.get_articles().await;
        assert_eq!(articles.len(), 1);
        assert_eq!(articles[0].path, "a.md");

        fs::remove_dir_all(&dir).unwrap();
    }

    #[tokio::test]
    async fn test_refresh_updates_cache() {
        let dir = temp_article_dir("refresh");
        fs::write(dir.join("a.md"), "# a").unwrap();

        let cache = ArticleCache::new(dir.clone());
        assert_eq!(cache.get_articles().await.len(), 1);

        // 新增與刪除
        fs::write(dir.join("b.md"), "# b").unwrap();
        fs::remove_file(dir.join("a.md")).unwrap();

        cache.refresh().await;
        let articles = cache.get_articles().await;
        assert_eq!(articles.len(), 1);
        assert_eq!(articles[0].path, "b.md");

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_is_relevant_event() {        let mk = |kind: EventKind, path: &Path| Event {
            kind,
            paths: vec![path.to_path_buf()],
            attrs: notify::event::EventAttributes::default(),
        };

        // .md 檔案的建立 / 修改 / 移除 → 觸發
        assert!(is_relevant_event(&mk(
            EventKind::Create(CreateKind::File),
            Path::new("/x/a.md")
        )));
        assert!(is_relevant_event(&mk(
            EventKind::Modify(ModifyKind::Data(DataChange::Any)),
            Path::new("/x/a.md")
        )));
        assert!(is_relevant_event(&mk(
            EventKind::Remove(RemoveKind::File),
            Path::new("/x/a.md")
        )));
        // 非 .md 檔案 → 不觸發
        assert!(!is_relevant_event(&mk(
            EventKind::Create(CreateKind::File),
            Path::new("/x/a.txt")
        )));
        // Access 事件 → 不觸發
        assert!(!is_relevant_event(&mk(
            EventKind::Access(AccessKind::Close(AccessMode::Any)),
            Path::new("/x/a.md")
        )));
    }

    /// refresh hook：每次 refresh 後以最新文章列表呼叫（驅動搜尋索引重建）
    #[tokio::test]
    async fn test_refresh_hook_called_with_latest_articles() {
        let dir = temp_article_dir("hook");
        fs::write(dir.join("a.md"), "# a").unwrap();
        let cache = Arc::new(ArticleCache::new(dir.clone()));

        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<usize>();
        cache.set_refresh_hook(move |articles| {
            let _ = tx.send(articles.len());
        });

        // 新增文章後 refresh → hook 收到最新數量（2）
        fs::write(dir.join("b.md"), "# b").unwrap();
        cache.refresh().await;
        assert_eq!(rx.try_recv().unwrap(), 2);

        // 快取本身也同步更新
        assert_eq!(cache.get_articles().await.len(), 2);

        fs::remove_dir_all(&dir).unwrap();
    }
}
