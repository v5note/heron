(() => {
  const root = document.documentElement;

  const I18N = {
    zh: {
      htmlLang: "zh-CN",
      docTitle: "Heron 苍鹭 — 把散落在各处的 Agent Skill 收拢成单一真实源",
      docDesc: "苍鹭静立一处，注视整片水面。Heron 是一款跨平台本地 Agent Skill 管理器：扫描 12+ AI 编码客户端，安全收拢至受 Git 版本管理的单一真实源，按需单向投影，并以三方合并安全升级。",
      langBtn: "EN",
      themeToLight: "切换浅色",
      themeToDark: "切换深色",
      modeDark: "深色",
      modeLight: "浅色",
      previewPrefix: "当前预览：",
      copiedPrefix: "已复制：",
      themeSwitched: "已切换主题：",
      themes: {
        "forest-ink": "林墨",
        default: "默认暖纸",
        dusk: "暮色",
        wanqing: "晚晴"
      },
      text: {
        "brand.name": "Heron 苍鹭",
        "nav.topology": "一源拓扑",
        "nav.workflow": "工作流",
        "nav.modes": "投影机制",
        "nav.clients": "12+ 客户端",
        "nav.contract": "安全契约",
        "nav.cli": "CLI & 下载",
        "nav.download": "下载桌面端",
        "hero.cta": "下载 macOS / Windows 安装包",
        "readout.n1": "1 份",
        "readout.l1": "单一真实源 · 独立 Git 仓库",
        "readout.l2": "内置主流 AI 客户端落点感知",
        "readout.n3": "4 种",
        "readout.l4": "Tauri 2 原生体积 · 零运行时依赖",
        "tabs.home": "概览 · 行动中心",
        "tabs.library": "我的 Skill · 落点矩阵",
        "tabs.discover": "发现 · 批量收拢",
        "tabs.market": "来源与市场",
        "tabs.updates": "更新 · 三方合并",
        "tabs.doctor": "体检 · 只读诊断",
        "tabs.settings": "设置 · 四套主题",
        "showcase.note": "真实运行截图 · 深浅随主题实时联动",
        "topo.title": "一源多投：看清磁盘上真正发生了什么",
        "topo.desc": "点击切换「收拢 / 分发 / 升级」三种状态，直观查看散落目录、单一真实源与 12+ AI 客户端之间的单向数据流向。",
        "topo.tab1": "01 收拢 Adopt",
        "topo.tab2": "02 分发 Project",
        "topo.tab3": "03 升级 Upgrade",
        "topo.leftTitle": "散落来源 · 磁盘现有拷贝",
        "topo.l1Tag": "未纳管",
        "topo.l2Tag": "重复 45",
        "topo.l3Name": "github.com · 5 大市场",
        "topo.l3Tag": "上游",
        "topo.coreTitle": "Heron 统一真相源 (Store)",
        "topo.rightTitle": "按需投影 · 12+ AI 客户端",
        "topo.r2Name": "Windows 落点",
        "wf.title": "收拢 · 分发 · 升级，一条清晰闭环",
        "wf.desc": "告别在五个隐藏目录间手工复制粘贴。GUI 与 CLI 共用同一套 Rust 业务内核，遵循「计算计划 → 预览确认 → 备份执行」铁律。",
        "wf.s1Title": "安全收拢，哈希去重",
        "wf.s1Desc": "多线程只读扫描本机已知路径与工作区。收拢前强制全量备份，Blake3 指纹自动合并重复拷贝，原位无缝替换为受管链接。",
        "wf.s2Title": "单向投影，显式开关",
        "wf.s2Desc": "从唯一存储区按需分发到指定客户端或项目目录。每个 Skill 在每个落点是否真正生效（生效中 / 被压制 / 坏链 / 需重启）一目了然。",
        "wf.s3Title": "三方合并，不丢定制",
        "modes.title": "四种投影模式，直面真实平台的边界",
        "modes.desc": "装得上不等于客户端看得见。Heron 显式区分四种落点机制，在 macOS 与 Windows 上均无需管理员提权。",
        "modes.m1Title": "符号链接",
        "modes.m1Desc": "macOS / Linux 首选。单点修改处处生效，零额外磁盘占用。",
        "modes.m2Os": "Windows 免提权",
        "modes.m2Title": "目录连接点",
        "modes.m2Desc": "Windows 专属。普通用户账户直接创建，彻底告别管理员权限弹窗。",
        "modes.m3Os": "代理 SKILL.md",
        "modes.m3Title": "指针文件",
        "modes.m4Os": "E_DRIFTED 防护",
        "modes.m4Title": "单向副本",
        "clients.title": "12+ AI 客户端，诚实探测",
        "clients.desc": "通过目录结构、CLI 版本与 App Bundle 复合信号探测本机安装状态。未安装的客户端只置灰标注，绝不伪造「生效」，绝不擅改配置。",
        "clients.live": "生效中",
        "clients.shadowed": "被压制",
        "clients.broken": "坏链",
        "clients.notScanned": "未扫描",
        "clients.agents": ".agents 共享",
        "contract.title": "首要铁律：从不擅自破坏你的既有数据",
        "contract.desc": "所有危险边界都写进编译期契约与错误码，而非靠文档口头提醒。",
        "contract.c1Title": "先备份，再动文件",
        "contract.c1Desc": "任何收拢或变更前强制完整备份至独立数据目录；备份失败则事务立刻中止。",
        "contract.c2Title": "拒绝隐式覆盖",
        "contract.c2Desc": "投影目标遇到未经纳管的非空目录时立即停下报错，绝不强制删除替换。",
        "contract.c3Title": "病态嵌套只诊断不盲动",
        "contract.c3Desc": "体检全程只读，精准定位重复、坏链与恶性嵌套，每一步修复都由你确认。",
        "contract.c4Title": "团队协同，一条命令复现",
        "cli.title": "桌面端之外，完整的同源命令行",
        "cli.copyBtn": "复制命令",
        "dl.mac": "下载 .dmg",
        "dl.win": "下载安装器",
        "footer.copy": "Heron 苍鹭 · 本地 Agent Skill 管理器 · 静立一处，注视整片水面",
        "footer.guide": "上手指南"
      },
      html: {
        "hero.title": "把散落在各处的 Agent Skill，<br />收拢成<em>一份受版本管理的真实源</em>",
        "hero.lead": "苍鹭静立一处，注视整片水面。扫描本机 12+ AI 编码客户端，去重收拢至 <span class=\"mono\">~/.heron/skills/</span>，按需单向投影，并以 Git 三方合并安全升级。",
        "wf.s3Desc": "每个 Skill 独立 Git 仓库。升级基于 <span class=\"mono\">git merge-tree</span> 内存级三方合并，保留本地提示词微调，冲突时逐文件可视化解决，随时一键回滚。",
        "modes.m3Desc": "生成轻量代理 <span class=\"mono\">SKILL.md</span>，攻克部分扫描器忽略 symlink 目录的缺陷。",
        "modes.m4Desc": "全平台兜底。内置 <span class=\"mono\">E_DRIFTED</span> 漂移检测，绝不静默覆盖外部手改内容。",
        "contract.c4Desc": "导出项目级锁文件，新人克隆仓库后 <span class=\"mono\">heron lock apply</span> 即可精准对齐整套 Skill。",
        "cli.desc": "所有命令支持 <span class=\"mono\">--json</span> 机器可读输出与 <span class=\"mono\">--data-dir</span> 沙箱隔离，天然适配脚本与 CI。",
        "dl.note": "环境要求：本机需安装 <span class=\"mono\">Git 2.38+</span>（用于 <span class=\"mono\">git merge-tree</span> 三方合并）。首次打开未签名安装包放行指引见 <a href=\"https://github.com/v5note/heron/blob/main/docs/SIGNING.md\" target=\"_blank\" rel=\"noopener\">docs/SIGNING.md</a>。"
      },
      cliPre: `<span class="t-dim"># 1. 扫描本机并预览收拢计划（绝不盲动）</span>
<span class="t-acc">$</span> heron scan
<span class="t-acc">$</span> heron adopt --all --dry-run

<span class="t-dim"># 2. 从任意 Git 仓库或 5 大内置市场安装</span>
<span class="t-acc">$</span> heron install https://github.com/owner/repo/tree/main/skills/code-review

<span class="t-dim"># 3. 团队锁文件导出与精准同步</span>
<span class="t-acc">$</span> heron lock export --write
<span class="t-acc">$</span> heron lock apply
<span class="t-good">✓ 48 skills managed · 216/216 projections verified · 0 drifted</span>`,
      shots: {
        home: "概览只列出此刻真正需要你决定的事：坏链、未纳管发现、各客户端真实生效状态。",
        library: "资产全景看板：一份实体，多个落点。支持列表/网格双视图与逐客户端一键投影开关。",
        discover: "扫描磁盘上已存在但尚未接管的 Skill，支持 Dry-Run 预览与一键安全收拢。",
        market: "一个搜索框打到所有启用来源：内置 skills.sh、ClawHub 等 5 大市场与任意 Git 仓库直装。",
        updates: "三方合并升级：清晰对比安装基线与上游提交，保留本地定制，冲突逐文件可视化解决。",
        doctor: "深度只读体检：精准定位坏链、存储缺失、病态嵌套与重复拷贝，所有修复均需确认。",
        settings: "内置默认、林墨、暮色、晚晴四套东方主题与深浅双模式，默认投影方式与代理一站配置。"
      },
      flows: {
        adopt: {
          core: "先全量备份，Blake3 哈希去重，每个 Skill 独立 Git 仓库（main / upstream / v* tag）。",
          summary: "收拢后原有路径保持不变，原地转换为指向唯一真实源的受管投影，绝不破坏既有工程。"
        },
        project: {
          core: "根据平台与客户端能力表自动选择 symlink / junction / pointer / copy，并执行可见性验证。",
          summary: "显式指定目标落点，绝不隐式猜测；遇到未纳管非空目录自动报 E_NOT_MANAGED 停机保护。"
        },
        upgrade: {
          core: "调用 git merge-tree 在内存中完成三方合并，上游新特性与本地提示词定制同时保留。",
          summary: "存在冲突时不污染工作区，提供左右并排可视化合并界面，并支持随时 heron rollback 回滚。"
        }
      }
    },
    en: {
      htmlLang: "en",
      docTitle: "Heron — Gather Scattered Agent Skills into One Single Source of Truth",
      docDesc: "Stand still, watch the whole water. Heron gathers Agent Skills scattered across 12+ AI coding tools into one Git-versioned store, projects them where your clients look, and upgrades via three-way merge.",
      langBtn: "中文",
      themeToLight: "Light",
      themeToDark: "Dark",
      modeDark: "Dark",
      modeLight: "Light",
      previewPrefix: "Preview: ",
      copiedPrefix: "Copied: ",
      themeSwitched: "Theme: ",
      themes: {
        "forest-ink": "Forest Ink",
        default: "Warm Paper",
        dusk: "Dusk",
        wanqing: "Wanqing"
      },
      text: {
        "brand.name": "Heron",
        "nav.topology": "Topology",
        "nav.workflow": "Workflow",
        "nav.modes": "Projection",
        "nav.clients": "12+ Clients",
        "nav.contract": "Safety",
        "nav.cli": "CLI & Download",
        "nav.download": "Download",
        "hero.cta": "Download for macOS / Windows",
        "readout.n1": "1 Store",
        "readout.l1": "Single source of truth · Per-Skill Git",
        "readout.l2": "Built-in AI client presence probing",
        "readout.n3": "4 Modes",
        "readout.l4": "Tauri 2 native bundle · Zero runtime deps",
        "tabs.home": "Overview",
        "tabs.library": "My Skills",
        "tabs.discover": "Discover",
        "tabs.market": "Sources & Markets",
        "tabs.updates": "Upgrades",
        "tabs.doctor": "Doctor",
        "tabs.settings": "Settings",
        "showcase.note": "Real app screenshots · Synced with Light & Dark themes",
        "topo.title": "One Store, Many Projections: See What Happens on Disk",
        "topo.desc": "Switch between Adopt, Project, and Upgrade to trace the unidirectional data flow across scattered folders, the central Git store, and 12+ AI clients.",
        "topo.tab1": "01 Adopt",
        "topo.tab2": "02 Project",
        "topo.tab3": "03 Upgrade",
        "topo.leftTitle": "SCATTERED SOURCES · ON DISK",
        "topo.l1Tag": "unowned",
        "topo.l2Tag": "45 dupes",
        "topo.l3Name": "github.com · 5 Markets",
        "topo.l3Tag": "upstream",
        "topo.coreTitle": "Heron Central Store",
        "topo.rightTitle": "ON-DEMAND PROJECTION · 12+ CLIENTS",
        "topo.r2Name": "Windows targets",
        "wf.title": "Adopt · Project · Upgrade — One Closed Loop",
        "wf.desc": "Stop copying folders across five hidden directories. Desktop GUI and CLI share one Rust service core enforcing Plan → Preview (dry-run) → Backup & Apply.",
        "wf.s1Title": "Safe Adoption & Deduplication",
        "wf.s1Desc": "Multi-threaded read-only scan across global and project paths. Full backup before any mutation, Blake3 hash deduplication, and in-place replacement with managed links.",
        "wf.s2Title": "Unidirectional Projection",
        "wf.s2Desc": "Project from the store to explicit client or workspace targets. Verify real client visibility at a glance: live, shadowed, broken, or restart required.",
        "wf.s3Title": "Three-Way Merge Upgrades",
        "modes.title": "Four Projection Modes Built for Real Platforms",
        "modes.desc": "Being on disk does not guarantee your client loads it. Heron supports four explicit projection modes—with zero admin elevation on macOS and Windows.",
        "modes.m1Title": "Symbolic Link",
        "modes.m1Desc": "Default on macOS / Linux. Edit once, reflect everywhere with zero duplicate disk usage.",
        "modes.m2Os": "Windows · No Admin",
        "modes.m2Title": "Directory Junction",
        "modes.m2Desc": "Windows native. Works under standard user accounts—no Administrator prompt or Developer Mode needed.",
        "modes.m3Os": "Proxy SKILL.md",
        "modes.m3Title": "Pointer File",
        "modes.m4Os": "E_DRIFTED Guard",
        "modes.m4Title": "Unidirectional Copy",
        "clients.title": "Honest Presence Detection for 12+ AI Clients",
        "clients.desc": "Probes installation via config directories, CLI --version, and macOS .app bundles. Undetected clients stay honestly greyed out—never faking live status or mutating switches.",
        "clients.live": "Live",
        "clients.shadowed": "Shadowed",
        "clients.broken": "Broken",
        "clients.notScanned": "Not scanned",
        "clients.agents": ".agents shared",
        "contract.title": "Prime Directive: Never Destroy User Data",
        "contract.desc": "Every safety boundary is enforced in Rust service contracts and structured error codes, not just documentation.",
        "contract.c1Title": "Backup Before Mutation",
        "contract.c1Desc": "Every adoption or destructive move writes a full backup to the isolated data directory first; if backup fails, the transaction aborts immediately.",
        "contract.c2Title": "Zero Silent Overwrites",
        "contract.c2Desc": "If a projection target already holds an unmanaged non-empty directory, Heron halts with E_NOT_MANAGED instead of force-deleting it.",
        "contract.c3Title": "Read-Only Doctor Diagnostics",
        "contract.c3Desc": "Doctor inspects duplicates, broken links, and pathological nested folders in read-only mode—every remediation requires your explicit approval.",
        "contract.c4Title": "Reproducible Team Lockfile",
        "cli.title": "Full Behavioral Parity in the CLI",
        "cli.copyBtn": "Copy command",
        "dl.mac": "Download .dmg",
        "dl.win": "Download installer",
        "footer.copy": "Heron · Local Agent Skill Manager · Stand still, watch the whole water",
        "footer.guide": "Guide"
      },
      html: {
        "hero.title": "Gather scattered Agent Skills into<br /><em>one versioned single source of truth</em>",
        "hero.lead": "Stand still, watch the whole water. Scan 12+ AI coding tools, consolidate into <span class=\"mono\">~/.heron/skills/</span>, project on demand, and upgrade cleanly with Git three-way merge.",
        "wf.s3Desc": "Every skill is an isolated Git repo. Upgrades run in-memory <span class=\"mono\">git merge-tree</span> three-way merges—keeping local prompt tweaks, surfacing visual conflict diffs, and supporting one-click rollbacks.",
        "modes.m3Desc": "Emits a lightweight proxy <span class=\"mono\">SKILL.md</span> to bypass client scanners where symlinked directories fail <span class=\"mono\">isDirectory()</span>.",
        "modes.m4Desc": "Cross-platform fallback equipped with <span class=\"mono\">E_DRIFTED</span> detection—refusing to silently overwrite external edits.",
        "contract.c4Desc": "Export <span class=\"mono\">heron.lock</span> at the project root so teammates can run <span class=\"mono\">heron lock apply</span> to mirror the exact skill set.",
        "cli.desc": "Every command supports <span class=\"mono\">--json</span> for scripts and CI plus <span class=\"mono\">--data-dir</span> for sandboxed execution.",
        "dl.note": "Requirement: <span class=\"mono\">Git 2.38+</span> (needed for <span class=\"mono\">git merge-tree</span>). First-launch instructions for unsigned builds: <a href=\"https://github.com/v5note/heron/blob/main/docs/SIGNING.md\" target=\"_blank\" rel=\"noopener\">docs/SIGNING.md</a>."
      },
      cliPre: `<span class="t-dim"># 1. Scan your machine and preview the adopt plan (never blind)</span>
<span class="t-acc">$</span> heron scan
<span class="t-acc">$</span> heron adopt --all --dry-run

<span class="t-dim"># 2. Install from any Git repo, monorepo subdir, or 5 built-in markets</span>
<span class="t-acc">$</span> heron install https://github.com/owner/repo/tree/main/skills/code-review

<span class="t-dim"># 3. Export and reproduce identical team environments</span>
<span class="t-acc">$</span> heron lock export --write
<span class="t-acc">$</span> heron lock apply
<span class="t-good">✓ 48 skills managed · 216/216 projections verified · 0 drifted</span>`,
      shots: {
        home: "Overview shows only what needs your decision right now: broken links, unowned discoveries, and live client target status.",
        library: "Asset inventory: one entity, many targets. Switch between list and grid views and toggle per-client projections in a drawer.",
        discover: "Inspect skills found on disk that are not yet managed by Heron, with Dry-Run previews and batch adoption.",
        market: "One search bar across all enabled sources: skills.sh, ClawHub, 5 built-in marketplaces, and direct Git URLs.",
        updates: "Three-way merge upgrades: compare installed baselines against upstream commits while preserving your local customizations.",
        doctor: "Read-only health diagnostics for broken links, missing stores, duplicates, and nested paths—every fix requires confirmation.",
        settings: "Four built-in palettes (Default, Forest Ink, Dusk, Wanqing) with Light/Dark modes, default projection mode, and proxy settings."
      },
      flows: {
        adopt: {
          core: "Full backup first, Blake3 hash deduplication, and an isolated Git repo per skill (main / upstream / v* tags).",
          summary: "Original paths remain intact—converted in place into managed projections pointing to your single source of truth."
        },
        project: {
          core: "Selects symlink / junction / pointer / copy per platform capability table and audits real client loading visibility.",
          summary: "Requires explicit targets with zero guesswork; stops with E_NOT_MANAGED if an unmanaged directory already exists."
        },
        upgrade: {
          core: "Runs in-memory git merge-tree three-way merges so upstream updates and local prompt tweaks coexist cleanly.",
          summary: "Conflicts never dirty your working tree—resolve side-by-side in the UI or roll back anytime with heron rollback."
        }
      }
    }
  };

  const SHOT_FILES = {
    home: { dark: "./assets/shot-home-dark.png", light: "./assets/shot-home-light.png" },
    library: { dark: "./assets/shot-library-dark.png", light: "./assets/shot-library-light.png" },
    discover: { dark: "./assets/shot-discover-dark.png", light: "./assets/shot-discover-light.png" },
    market: { dark: "./assets/shot-market-dark.png", light: "./assets/shot-market-light.png" },
    updates: { dark: "./assets/shot-updates-dark.png", light: "./assets/shot-updates-light.png" },
    doctor: { dark: "./assets/shot-doctor-dark.png", light: "./assets/shot-doctor-light.png" },
    settings: { dark: "./assets/shot-settings-dark.png", light: "./assets/shot-settings-light.png" }
  };

  const FLOW_META = {
    adopt: {
      cmd: "$ heron adopt --all --dry-run",
      copy: "heron adopt --all --dry-run",
      lit: ["l1", "l2"],
      arrows: ["arr1"]
    },
    project: {
      cmd: "$ heron project ask-matt --on claude --on cursor",
      copy: "heron project ask-matt --on claude --on cursor",
      lit: ["r1", "r2", "r3"],
      arrows: ["arr2"]
    },
    upgrade: {
      cmd: "$ heron upgrade --all && heron lock export --write",
      copy: "heron upgrade --all && heron lock export --write",
      lit: ["l3", "r1", "r2"],
      arrows: ["arr1", "arr2"]
    }
  };

  // Initial state: honor ?lang=en|zh first, then localStorage, then default zh
  const params = new URLSearchParams(window.location.search);
  const urlLang = params.get("lang");
  let currentLang = (urlLang === "en" || urlLang === "zh")
    ? urlLang
    : (localStorage.getItem("heron.site.lang") || "zh");

  const savedTheme = localStorage.getItem("heron.site.theme") || "forest-ink";
  const savedMode = localStorage.getItem("heron.site.mode") || "dark";
  root.setAttribute("data-theme", savedTheme);
  root.setAttribute("data-color-mode", savedMode);

  let currentView = "home";
  let currentFlow = "adopt";

  const shotImg = document.getElementById("shotImg");
  const shotDesc = document.getElementById("shotDesc");
  const shotThemeLabel = document.getElementById("shotThemeLabel");
  const themeToggle = document.getElementById("themeToggle");
  const langToggle = document.getElementById("langToggle");
  const toastEl = document.getElementById("toast");
  let toastTimer = null;

  function showToast(msg) {
    toastEl.textContent = msg;
    toastEl.classList.add("show");
    clearTimeout(toastTimer);
    toastTimer = setTimeout(() => toastEl.classList.remove("show"), 2000);
  }

  function copyText(text) {
    const dict = I18N[currentLang];
    if (navigator.clipboard && navigator.clipboard.writeText) {
      navigator.clipboard.writeText(text).then(() => showToast(`${dict.copiedPrefix}${text}`));
    } else {
      showToast(text);
    }
  }

  function renderShowcase() {
    const dict = I18N[currentLang];
    const theme = root.getAttribute("data-theme");
    const mode = root.getAttribute("data-color-mode");
    shotImg.src = SHOT_FILES[currentView][mode];
    shotDesc.textContent = dict.shots[currentView];
    const modeText = mode === "dark" ? dict.modeDark : dict.modeLight;
    shotThemeLabel.textContent = `${dict.previewPrefix}${dict.themes[theme] || dict.themes["forest-ink"]} · ${modeText}`;
    themeToggle.textContent = mode === "dark" ? dict.themeToLight : dict.themeToDark;

    document.querySelectorAll(".p-dot").forEach((dot) => {
      dot.classList.toggle("active", dot.dataset.theme === theme);
    });
  }

  function renderFlow(mode) {
    currentFlow = mode;
    const meta = FLOW_META[mode];
    const copy = I18N[currentLang].flows[mode];
    document.querySelectorAll(".flow-btn").forEach((b) => {
      b.classList.toggle("active", b.dataset.mode === mode);
    });
    const cmdBtn = document.getElementById("flowCmd");
    cmdBtn.textContent = meta.cmd;
    cmdBtn.dataset.copy = meta.copy;
    document.getElementById("coreDesc").textContent = copy.core;
    document.getElementById("flowSummary").textContent = copy.summary;

    ["l1", "l2", "l3", "r1", "r2", "r3"].forEach((id) => {
      document.getElementById(id).classList.toggle("lit", meta.lit.includes(id));
    });
    ["arr1", "arr2"].forEach((id) => {
      document.getElementById(id).classList.toggle("active", meta.arrows.includes(id));
    });
  }

  function renderLanguage() {
    const dict = I18N[currentLang];
    root.setAttribute("lang", dict.htmlLang);
    document.title = dict.docTitle;
    const metaDesc = document.querySelector('meta[name="description"]');
    if (metaDesc) metaDesc.setAttribute("content", dict.docDesc);

    langToggle.textContent = dict.langBtn;

    document.querySelectorAll("[data-i18n]").forEach((el) => {
      const key = el.dataset.i18n;
      if (dict.text[key] !== undefined) el.textContent = dict.text[key];
    });
    document.querySelectorAll("[data-i18n-html]").forEach((el) => {
      const key = el.dataset.i18nHtml;
      if (dict.html[key] !== undefined) el.innerHTML = dict.html[key];
    });
    document.getElementById("cliPre").innerHTML = dict.cliPre;

    renderShowcase();
    renderFlow(currentFlow);
  }

  // View tabs
  document.getElementById("viewTabs").addEventListener("click", (e) => {
    const btn = e.target.closest(".tab-btn");
    if (!btn) return;
    document.querySelectorAll(".tab-btn").forEach((b) => b.classList.remove("active"));
    btn.classList.add("active");
    currentView = btn.dataset.view;
    renderShowcase();
  });

  // Language toggle
  langToggle.addEventListener("click", () => {
    currentLang = currentLang === "zh" ? "en" : "zh";
    localStorage.setItem("heron.site.lang", currentLang);
    renderLanguage();
  });

  // Theme light/dark toggle
  themeToggle.addEventListener("click", () => {
    const next = root.getAttribute("data-color-mode") === "dark" ? "light" : "dark";
    root.setAttribute("data-color-mode", next);
    localStorage.setItem("heron.site.mode", next);
    renderShowcase();
  });

  // 4-palette selector
  document.getElementById("paletteDots").addEventListener("click", (e) => {
    const dot = e.target.closest(".p-dot");
    if (!dot) return;
    const next = dot.dataset.theme;
    root.setAttribute("data-theme", next);
    localStorage.setItem("heron.site.theme", next);
    renderShowcase();
    const dict = I18N[currentLang];
    showToast(`${dict.themeSwitched}${dict.themes[next]}`);
  });

  // Topology flow switch
  document.getElementById("flowSwitch").addEventListener("click", (e) => {
    const btn = e.target.closest(".flow-btn");
    if (!btn) return;
    renderFlow(btn.dataset.mode);
  });

  // Click-to-copy handlers
  document.querySelectorAll("[data-copy]").forEach((el) => {
    el.addEventListener("click", (e) => {
      if (el.tagName === "A") e.preventDefault();
      copyText(el.dataset.copy);
    });
  });

  document.getElementById("flowCmd").addEventListener("click", (e) => {
    const cmd = e.currentTarget.dataset.copy;
    if (cmd) copyText(cmd);
  });

  renderLanguage();
})();
