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
let allArticles = [];
// 由 /api/config 提供；預設 false（安全）：文章中的 HTML 會被 DOMPurify 清除
let allowMarkdownHtml = false;

init();

async function init() {
    setupFAB();
    setupSettings();
    setupShareButton();
    setupSearch();
    setupSwipeGestures();
    if (window.innerWidth < 768) {
        sidebar.classList.add('collapsed');
    }
    await Promise.all([loadConfig(), loadArticleIndex()]);
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
    } catch { /* 保持預設 false */ }
}

async function loadArticleIndex() {
    articleListElement.innerHTML = '<li style="color:#888;">載入中...</li>';
    try {
        const response = await fetch('/api/articles');
        if (!response.ok) throw new Error('無法載入文章索引');
        const data = await response.json();
        allArticles = data.articles || [];
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

function buildTree(articles) {
    const root = {};
    for (const article of articles) {
        const parts = article.path.split('/');
        let node = root;
        parts.forEach((part, index) => {
            const isFile = index === parts.length - 1;
            if (isFile) {
                node[part] = {
                    type: 'file',
                    article
                };
            } else {
                node[part] ??= {
                    type: 'folder',
                    children: {}
                };
                node = node[part].children;
            }
        });
    }
    return root;
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
    const tree = buildTree(allArticles);
    renderTree(articleListElement, tree);
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
            const article = allArticles.find(a => a.path === r.path);
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
        if (!path) return;
        const decoded = decodeURIComponent(path);
        const article = allArticles.find(a => a.path === decoded);
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

// ── 渲染 Markdown ─────────────────────────────
async function loadMarkdown(article) {
    const url = new URL(location);
    url.searchParams.set('article', article.path);
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
        const rendered = marked.parse(markdownText);
        articleBody.innerHTML = allowMarkdownHtml
            ? rendered
            : DOMPurify.sanitize(rendered);

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