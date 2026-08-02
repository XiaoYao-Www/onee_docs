// 取得 DOM 元素
const sidebar = document.getElementById('sidebar');
const btnTheme = document.getElementById('btn-theme');
const btnFontInc = document.getElementById('btn-font-inc');
const btnFontDec = document.getElementById('btn-font-dec');
const articleListElement = document.getElementById('article-list');
const tocListElement = document.getElementById('toc-list');
const articleBody = document.getElementById('article-body');

// 取得 FAB 相關元素
const fabContainer = document.getElementById('fab-container');
const fabToggle = document.getElementById('fab-toggle');
const fabSidebarToggle = document.getElementById('fab-sidebar-toggle');
const fabSearch = document.getElementById('fab-search');
const articleBreadcrumb = document.getElementById('article-breadcrumb');
const btnShare = document.getElementById('btn-share');
const shareIconContainer = document.getElementById('share-icon-container');

// 取得搜尋相關元素
const searchOverlay = document.getElementById('search-overlay');
const searchInput = document.getElementById('search-input');
const searchClose = document.getElementById('search-close');
const searchResults = document.getElementById('search-results');

// SVG 圖示定義
const ICON_LINK = `<svg xmlns="http://www.w3.org/2000/svg" width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M10 13a5 5 0 0 0 7.54.54l3-3a5 5 0 0 0-7.07-7.07l-1.72 1.71"></path><path d="M14 11a5 5 0 0 0-7.54-.54l-3 3a5 5 0 0 0 7.07 7.07l1.71-1.71"></path></svg>`;
const ICON_CHECK = `<svg xmlns="http://www.w3.org/2000/svg" width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round"><polyline points="20 6 9 17 4 12"></polyline></svg>`;

let currentFontSize = 16;
let allArticles = {};
// 根文章（首頁）：由 /api/articles 的 home 欄位提供；不存在時為 null
let homeArticle = null;
// 由 /api/config 提供；預設 false（安全）：文章中的 HTML 會被 DOMPurify 清除
let allowMarkdownHtml = false;

init();

async function init() {
    setupFAB();
    setupSettings();
    setupShareButton();
    setupSearch();
    setupSwipeGestures();
    setupSiteTitleHome();
    if (window.innerWidth < 768) {
        sidebar.classList.add('collapsed');
    }
    await Promise.all([loadConfig(), loadArticleIndex()]);
}

// 點擊頂部站點標題 → 回到根文章（首頁）；無根文章時不作用
function setupSiteTitleHome() {
    const siteTitle = document.getElementById('site-title');
    if (!siteTitle) return;
    siteTitle.addEventListener('click', () => {
        if (!homeArticle) return;
        loadMarkdown(homeArticle);
    });
}

// ── 手機右滑/左滑偵測 ─────────────────────────────
function setupSwipeGestures() {
    let touchStartX = 0;
    let touchEndX = 0;
    const swipeThreshold = 50;
    const edgeThreshold = 40;

    document.addEventListener('touchstart', e => {
        touchStartX = e.changedTouches[0].screenX;
    }, { passive: true });

    document.addEventListener('touchend', e => {
        touchEndX = e.changedTouches[0].screenX;
        handleSwipe();
    }, { passive: true });

    function handleSwipe() {
        if (window.innerWidth >= 768) return;
        const swipeDistance = touchEndX - touchStartX;
        if (swipeDistance > swipeThreshold && touchStartX < edgeThreshold) {
            sidebar.classList.remove('collapsed');
        } else if (swipeDistance < -swipeThreshold) {
            sidebar.classList.add('collapsed');
        }
    }
}

// 載入渲染設定
async function loadConfig() {
    try {
        const resp = await fetch('/api/config');
        if (!resp.ok) return;
        const data = await resp.json();
        allowMarkdownHtml = !!data.allow_markdown_html;

        // 標題設定：以設定檔為準，API 未提供時保留 HTML 內建值
        const siteTitleEl = document.getElementById('site-title');
        const pageTitleEl = document.getElementById('page-title');
        if (data.site_title && siteTitleEl) siteTitleEl.textContent = data.site_title;
        if (data.page_title) {
            document.title = data.page_title;
            if (pageTitleEl) pageTitleEl.textContent = data.page_title;
        }
    } catch { /* 保持預設 false */ }
}

async function loadArticleIndex() {
    articleListElement.innerHTML = '<li style="color:#888;">載入中...</li>';
    try {
        const response = await fetch('/api/articles');
        if (!response.ok) throw new Error('無法載入文章索引');
        const data = await response.json();
        allArticles = data.articles || {};
        // 根文章（首頁）：存在時建立文章物件（標題顯示「首頁」）
        homeArticle = data.home
            ? { title: '首頁', path: data.home, date: null }
            : null;
        renderArticleList();
        restoreArticleFromURL();
    } catch (error) {
        articleListElement.innerHTML =
            '<li style="color:red;">無法載入文章。<br>請確認啟動了 <code>python server.py</code></li>';
    }
}

// ── FAB 與側邊欄邏輯 ─────────────────────────────
function setupFAB() {
    fabToggle.addEventListener('click', (e) => {
        e.stopPropagation();
        fabContainer.classList.toggle('open');
    });

    fabSidebarToggle.addEventListener('click', () => {
        sidebar.classList.toggle('collapsed');
        if (window.innerWidth < 768 && !sidebar.classList.contains('collapsed')) {
            fabContainer.classList.remove('open');
        }
    });

    fabSearch.addEventListener('click', () => {
        searchOverlay.style.display = '';
        setTimeout(() => searchInput.focus(), 100);
        fabContainer.classList.remove('open');
    });

    window.addEventListener('click', (e) => {
        if (!fabContainer.contains(e.target)) {
            fabContainer.classList.remove('open');
        }
    });

    document.getElementById('main-content').addEventListener('click', () => {
        if (!sidebar.classList.contains('collapsed')) {
            sidebar.classList.add('collapsed');
        }
    });
}

function setupSettings() {
    btnTheme.addEventListener('click', () => {
        document.body.classList.toggle('dark-theme');
    });
    btnFontInc.addEventListener('click', () => {
        currentFontSize += 2;
        document.documentElement.style.setProperty('--base-font-size', `${currentFontSize}px`);
    });
    btnFontDec.addEventListener('click', () => {
        if (currentFontSize > 12) {
            currentFontSize -= 2;
            document.documentElement.style.setProperty('--base-font-size', `${currentFontSize}px`);
        }
    });
}

// 在樹狀結構中依 path 遞迴查找文章（後端已回傳樹，前端直接使用）
function findArticle(node, path) {
    for (const [name, item] of Object.entries(node)) {
        if (item.type === 'file') {
            if (item.article.path === path) return item.article;
        } else if (item.type === 'folder') {
            const found = findArticle(item.children, path);
            if (found) return found;
        }
    }
    return null;
}

// 將文章樹展平為扁平文章列表（title/path/date），供 onee_docs 區塊排序使用
function flattenArticles(node, out = []) {
    for (const item of Object.values(node)) {
        if (item.type === 'file') {
            out.push(item.article);
        } else if (item.type === 'folder') {
            flattenArticles(item.children, out);
        }
    }
    return out;
}

function renderTree(parent, node, depth = 0) {

    Object.entries(node).forEach(([name, item]) => {


        if (item.type === "folder") {


            const folder = document.createElement("div");

            folder.className = "tree-folder";

            folder.style.setProperty(
                "--depth",
                depth
            );



            // 標題
            const header = document.createElement("div");

            header.className = "folder-header";

            header.style.setProperty(
                "--depth",
                depth
            );



            const icon = document.createElement("span");

            icon.className = "folder-icon";

            icon.textContent = "▶";



            const label = document.createElement("span");

            label.textContent = name;



            header.append(
                icon,
                label
            );



            // 子內容
            const children = document.createElement("div");

            children.className =
                "folder-children";


            children.style.height =
                children.scrollHeight + "px";



            renderTree(
                children,
                item.children,
                depth + 1
            );



            // 點擊展開
            header.addEventListener("click", () => {

                const isOpen =
                    children.style.height !== "0px";


                if (isOpen) {

                    // 收合

                    children.style.height =
                        children.scrollHeight + "px";


                    requestAnimationFrame(() => {

                        children.style.height =
                            "0px";

                    });


                    icon.classList.remove("open");


                } else {

                    // 展開

                    children.style.height =
                        children.scrollHeight + "px";


                    icon.classList.add("open");


                    children.addEventListener(
                        "transitionend",
                        () => {

                            children.style.height = "";

                        },
                        {
                            once: true
                        }
                    );

                }

            });



            folder.append(
                header,
                children
            );


            parent.appendChild(folder);



        } else {



            const file =
                document.createElement("div");


            file.className =
                "tree-file";


            file.style.setProperty(
                "--depth",
                depth
            );



            const icon =
                document.createElement("span");

            icon.className =
                "file-icon";

            icon.textContent =
                "📄";



            const title =
                document.createElement("span");


            title.textContent =
                item.article.title;



            file.append(
                icon,
                title
            );



            file.addEventListener(
                "click",
                () => {
                    onArticleClick(
                        item.article
                    );
                }
            );


            parent.appendChild(file);

        }

    });

}

// ── 渲染文章列表 (實裝完美抽屜動畫) ─────────────────────────────
function renderArticleList() {
    articleListElement.innerHTML = "";
    // 後端已回傳樹狀結構：根為 folder 節點，渲染其 children
    renderTree(articleListElement, allArticles.children);
}

function setupShareButton() {
    if (!btnShare) return;
    btnShare.addEventListener('click', async () => {
        try {
            await navigator.clipboard.writeText(location.href);
        } catch {
            const textarea = document.createElement('textarea');
            textarea.value = location.href;
            document.body.appendChild(textarea);
            textarea.select();
            document.execCommand('copy');
            document.body.removeChild(textarea);
        }
        btnShare.classList.add('copied');
        shareIconContainer.innerHTML = ICON_CHECK;
        setTimeout(() => {
            btnShare.classList.remove('copied');
            shareIconContainer.innerHTML = ICON_LINK;
        }, 2000);
    });
}

// ── 搜尋功能 ─────────────────────────────
function setupSearch() {
    let debounceTimer = null;

    function closeSearch() {
        searchOverlay.style.display = 'none';
        searchInput.value = '';
        searchResults.innerHTML = '';
        searchResults.style.display = 'none';
    }

    searchClose.addEventListener('click', closeSearch);

    searchOverlay.addEventListener('click', (e) => {
        if (e.target === searchOverlay) closeSearch();
    });

    document.addEventListener('keydown', (e) => {
        if (e.key === 'Escape' && searchOverlay.style.display !== 'none') {
            closeSearch();
        }
    });

    searchInput.addEventListener('input', () => {
        clearTimeout(debounceTimer);
        const q = searchInput.value.trim();
        if (!q) {
            searchResults.innerHTML = '';
            searchResults.style.display = 'none';
            return;
        }
        debounceTimer = setTimeout(async () => {
            try {
                const resp = await fetch(`/api/search?q=${encodeURIComponent(q)}`);
                if (!resp.ok) throw new Error('搜尋失敗');
                const data = await resp.json();
                renderSearchResults(data.results || []);
            } catch {
                searchResults.innerHTML = '<div class="search-result-item error">搜尋失敗</div>';
                searchResults.style.display = 'block';
            }
        }, 300);
    });
}

function renderSearchResults(results) {
    searchResults.innerHTML = '';
    if (results.length === 0) {
        searchResults.innerHTML = '<div class="search-result-item empty">找不到相符的文章</div>';
        searchResults.style.display = 'block';
        return;
    }
    results.forEach(r => {
        const div = document.createElement('div');
        div.className = 'search-result-item';

        const titleLine = document.createElement('div');
        titleLine.className = 'search-result-title';
        const badge = r.match_in === 'title' ? '[標題] ' : r.match_in === 'content' ? '[內容] ' : '[全文] ';
        titleLine.textContent = `${r.date ? r.date + ' ' : ''}${badge}${r.title}`;
        div.appendChild(titleLine);

        if (r.snippet) {
            const snip = document.createElement('div');
            snip.className = 'search-result-snippet';
            snip.innerHTML = DOMPurify.sanitize(r.snippet, {
                ALLOWED_TAGS: ['mark'],
                ALLOWED_ATTR: [],
            });
            div.appendChild(snip);
        }

        div.addEventListener('click', () => {
            const article = findArticle(allArticles.children, r.path);
            if (article) {
                loadMarkdown(article);
                closeSearch();
            }
        });
        searchResults.appendChild(div);
    });
    searchResults.style.display = 'block';
}

function closeSearch() {
    searchOverlay.style.display = 'none';
    searchInput.value = '';
    searchResults.innerHTML = '';
    searchResults.style.display = 'none';
}

// ── 從 URL 參數恢復文章 ─────────────────────────────
function restoreArticleFromURL() {
    try {
        const params = new URLSearchParams(location.search);
        const path = params.get('article');
        // 未指定文章 → 顯示根文章（若有）
        if (!path) {
            if (homeArticle) loadMarkdown(homeArticle);
            return;
        }
        const decoded = decodeURIComponent(path);
        // 根文章不在列表樹中，需單獨比對
        if (homeArticle && decoded === homeArticle.path) {
            loadMarkdown(homeArticle);
            return;
        }
        const article = findArticle(allArticles.children, decoded);
        if (article) loadMarkdown(article);
    } catch { }
}

function onArticleClick(article) {
    loadMarkdown(article);
    if (window.innerWidth < 768) {
        sidebar.classList.add('collapsed');
        fabContainer.classList.remove('open');
    }
}

// ── onee_docs 特殊區塊（僅根文章 index.md 生效） ─────────────
// 語法：以 ```onee_docs 開頭、``` 結尾的 fenced code block，
// 內部每行一個 key = value 參數（# 後為注釋，值可加引號）：
//   sort = newest        # newest | oldest | random（預設 newest）
//   layout = list        # list | slide | grid（預設 list）
//   direction = vertical # 僅 layout=grid：vertical 縱向排列 | horizontal 橫向排列
//   count = 10           # 顯示數量（預設 10；0 或省略 = 全部）
//   title = 推薦閱讀      # 區塊標題（可選）
const ONEE_DOCS_BLOCK_RE = /```onee_docs\s*\n([\s\S]*?)```/g;

// 去掉行尾注釋：# 不在引號內時視為注釋起點
function stripInlineComment(line) {
    let inQuote = null;
    for (let i = 0; i < line.length; i++) {
        const ch = line[i];
        if (inQuote) {
            if (ch === inQuote) inQuote = null;
        } else if (ch === '"' || ch === "'") {
            inQuote = ch;
        } else if (ch === '#') {
            return line.slice(0, i);
        }
    }
    return line;
}

// 解析 onee_docs 區塊內文為參數物件；無效行忽略
function parseOneeDocsParams(blockText) {
    const params = {};
    for (let line of blockText.split('\n')) {
        line = stripInlineComment(line).trim();
        if (!line) continue;
        const eq = line.indexOf('=');
        if (eq === -1) continue;
        const key = line.slice(0, eq).trim();
        let value = line.slice(eq + 1).trim();
        // 去除成對引號
        if ((value.startsWith('"') && value.endsWith('"')) ||
            (value.startsWith("'") && value.endsWith("'"))) {
            value = value.slice(1, -1).trim();
        }
        if (key) params[key] = value;
    }
    return params;
}

// 提取 markdown 中所有 onee_docs 區塊，替換為佔位符（前後加空行確保獨立 block），
// 回傳 { markdown, blocks }；blocks[i].params 對應佔位符 data-onee-docs="i"
function extractOneeDocsBlocks(markdown) {
    const blocks = [];
    let index = 0;
    const replaced = markdown.replace(ONEE_DOCS_BLOCK_RE, (match, inner) => {
        blocks.push({ params: parseOneeDocsParams(inner) });
        return `\n\n<div data-onee-docs="${index++}"></div>\n\n`;
    });
    return { markdown: replaced, blocks };
}

// 依 sort 排序：newest 日期降冪、oldest 升冪、random 洗牌；
// 無日期文章一律排末尾（隨機時一併參與洗牌）
function sortArticlesForBlock(articles, sort) {
    if (sort === 'random') {
        const result = articles.slice();
        for (let i = result.length - 1; i > 0; i--) {
            const j = Math.floor(Math.random() * (i + 1));
            [result[i], result[j]] = [result[j], result[i]];
        }
        return result;
    }
    const withDate = [];
    const withoutDate = [];
    for (const a of articles) {
        (a.date ? withDate : withoutDate).push(a);
    }
    withDate.sort((a, b) =>
        sort === 'newest'
            ? b.date.localeCompare(a.date)
            : a.date.localeCompare(b.date)
    );
    return [...withDate, ...withoutDate];
}

// 渲染一個 onee_docs 區塊組件到 container（佔位符 div）內。
// 全部以 createElement + textContent 構建，不經 innerHTML，無 XSS 面。
function renderOneeDocsBlock(block, container) {
    const params = block.params;
    const sort = ['newest', 'oldest', 'random'].includes(params.sort) ? params.sort : 'newest';
    const layout = ['list', 'slide', 'grid'].includes(params.layout) ? params.layout : 'list';
    const direction = params.direction === 'horizontal' ? 'horizontal' : 'vertical';
    let count = parseInt(params.count, 10);
    if (!Number.isInteger(count) || count <= 0) count = Infinity;

    const items = sortArticlesForBlock(flattenArticles(allArticles.children), sort);
    const shown = count === Infinity ? items : items.slice(0, count);
    if (shown.length === 0) return;

    const blockEl = document.createElement('div');
    const gridDir = layout === 'grid' ? ` onee-docs-${direction}` : '';
    blockEl.className = `onee-docs-block onee-docs-${layout}${gridDir}`;

    if (params.title) {
        // 用 div 而非 heading，避免進入文章大綱（TOC）
        const titleEl = document.createElement('div');
        titleEl.className = 'onee-docs-title';
        titleEl.textContent = params.title;
        blockEl.appendChild(titleEl);
    }

    const listEl = document.createElement('div');
    listEl.className = 'onee-docs-items';
    blockEl.appendChild(listEl);

    shown.forEach(article => {
        const item = document.createElement('div');
        item.className = 'onee-docs-item';
        item.setAttribute('role', 'button');
        item.tabIndex = 0;

        if (article.date) {
            const dateEl = document.createElement('span');
            dateEl.className = 'onee-docs-date';
            dateEl.textContent = article.date;
            item.appendChild(dateEl);
        }

        const titleEl = document.createElement('span');
        titleEl.className = 'onee-docs-item-title';
        titleEl.textContent = article.title;
        item.appendChild(titleEl);

        item.addEventListener('click', () => loadMarkdown(article));
        item.addEventListener('keydown', (e) => {
            if (e.key === 'Enter' || e.key === ' ') {
                e.preventDefault();
                loadMarkdown(article);
            }
        });

        listEl.appendChild(item);
    });

    container.appendChild(blockEl);
}

// ── 渲染 Markdown ─────────────────────────────
async function loadMarkdown(article) {
    const url = new URL(location);
    if (homeArticle && article.path === homeArticle.path) {
        // 根文章（首頁）：清除 article 參數，保持乾淨的根路徑
        url.searchParams.delete('article');
    } else {
        url.searchParams.set('article', article.path);
    }
    history.replaceState(null, '', url);

    if (article.date) {
        articleBreadcrumb.textContent = `${article.date} › ${article.title}`;
    } else {
        articleBreadcrumb.textContent = article.title;
    }

    if (btnShare) btnShare.style.display = 'flex';

    articleBody.innerHTML = '讀取中...';
    tocListElement.innerHTML = '';

    try {
        const response = await fetch(`/api/article?path=${encodeURIComponent(article.path)}`);
        if (!response.ok) throw new Error('找不到檔案');

        const markdownText = await response.text();

        // 僅根文章解析 onee_docs 特殊區塊；其他文章原樣渲染（區塊顯示為普通代碼塊）
        let renderText = markdownText;
        let oneeBlocks = [];
        if (homeArticle && article.path === homeArticle.path) {
            const extracted = extractOneeDocsBlocks(markdownText);
            renderText = extracted.markdown;
            oneeBlocks = extracted.blocks;
        }

        const rendered = marked.parse(renderText);
        articleBody.innerHTML = allowMarkdownHtml
            ? rendered
            : DOMPurify.sanitize(rendered);

        // 將 onee_docs 區塊組件依序注入佔位符（佔位符若被淨化移除則跳過）
        if (oneeBlocks.length > 0) {
            articleBody.querySelectorAll('[data-onee-docs]').forEach(ph => {
                const idx = parseInt(ph.getAttribute('data-onee-docs'), 10);
                const block = oneeBlocks[idx];
                if (block) renderOneeDocsBlock(block, ph);
            });
        }

        generateTOC();
        document.getElementById('main-content').scrollTo({ top: 0, behavior: 'smooth' });

    } catch (error) {
        articleBody.innerHTML = '';
        const p = document.createElement('p');
        p.style.color = 'red';
        p.textContent = `載入失敗：請確認檔案 ${article.path} 是否存在。`;
        articleBody.appendChild(p);
    }
}

function generateTOC() {
    const headings = articleBody.querySelectorAll('h1, h2, h3, h4, h5, h6');

    if (headings.length === 0) {
        tocListElement.innerHTML = '<li style="color:#888;">無標題</li>';
        return;
    }

    headings.forEach((heading, index) => {
        const anchorId = `heading-${index}`;
        heading.id = anchorId;

        const level = parseInt(heading.tagName.substring(1));
        const indent = (level - 1) * 12;

        const li = document.createElement('li');
        li.style.marginLeft = `${indent}px`;

        const a = document.createElement('a');
        a.href = `#${anchorId}`;
        a.textContent = heading.textContent;

        li.appendChild(a);
        tocListElement.appendChild(li);
    });
}