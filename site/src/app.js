// The landing page's small bits of life. No framework: a handful of
// listeners, and every animation stops when the visitor prefers less motion.
(() => {
  const root = document.documentElement;
  const calm = matchMedia("(prefers-reduced-motion: reduce)").matches;
  const lang = () => root.dataset.lang;
  const t = (zh, en) => (lang() === "zh" ? zh : en);
  const $$ = (sel) => [...document.querySelectorAll(sel)];

  // Segmented controls share one look: the chosen one is lifted.
  function choose(buttons, on) {
    buttons.forEach((b) => {
      const yes = on(b);
      b.classList.toggle("bg-fg", yes);
      b.classList.toggle("text-bg", yes);
      b.classList.toggle("text-fg/55", !yes);
      b.setAttribute("aria-selected", yes);
    });
  }

  // Swap something on the page without a jolt: cross-fade where the browser
  // can, and keep whatever sits mid-screen exactly where it was.
  function smoothly(change, opts = {}) {
    const anchor = document.elementFromPoint(innerWidth / 2, innerHeight / 2);
    const before = anchor ? anchor.getBoundingClientRect().top : 0;
    const apply = () => {
      change();
      if (anchor && anchor.isConnected) scrollBy({ top: anchor.getBoundingClientRect().top - before, behavior: "instant" });
    };
    if (calm || !document.startViewTransition) return apply();
    if (opts.skin) root.classList.add("skin-change");
    const vt = document.startViewTransition(apply);
    vt.finished.finally(() => root.classList.remove("skin-change"));
  }

  // Skin: follows the system until the visitor picks one with the toggle.
  const system = matchMedia("(prefers-color-scheme: light)");
  const themeColor = document.querySelector('meta[name="theme-color"]');
  function paint(theme) {
    root.dataset.theme = theme;
    if (themeColor) themeColor.content = theme === "light" ? "#f3efe9" : "#0c0b0a";
  }
  system.addEventListener("change", () => {
    let saved = null;
    try { saved = localStorage.getItem("theme"); } catch (e) {}
    if (!saved) smoothly(() => paint(system.matches ? "light" : "dark"));
  });
  const themeBtn = document.getElementById("theme");
  themeBtn.addEventListener("click", () => {
    const next = root.dataset.theme === "light" ? "dark" : "light";
    // Picking what the system already says means "follow the system" again.
    try {
      if (next === (system.matches ? "light" : "dark")) localStorage.removeItem("theme");
      else localStorage.setItem("theme", next);
    } catch (e) {}
    const r = themeBtn.getBoundingClientRect();
    root.style.setProperty("--vx", r.left + r.width / 2 + "px");
    root.style.setProperty("--vy", r.top + r.height / 2 + "px");
    smoothly(() => paint(next), { skin: true });
  });
  paint(root.dataset.theme);

  // Language: every phrase is in the page twice; pictures swap their source.
  function setLang(next) {
    root.dataset.lang = next;
    root.lang = next === "zh" ? "zh-CN" : "en";
    try { localStorage.setItem("lang", next); } catch (e) {}
    $$(".lang-img").forEach((img) => {
      const src = img.dataset["src" + (next === "zh" ? "Zh" : "En")];
      if (src && img.getAttribute("src") !== src) img.src = src;
    });
    choose($$(".lang-btn"), (b) => b.dataset.setLang === next);
    document.title = t("cc-moss · 文明会延续，对话也是。", "cc-moss · Every session, carried on.");
    refreshMo();
  }
  $$("[data-set-lang]").forEach((b) => b.addEventListener("click", () => {
    if (b.dataset.setLang !== lang()) smoothly(() => setLang(b.dataset.setLang));
  }));
  // Fetch the other language's pictures once the page is idle, so a switch never shows a blank.
  addEventListener("load", () => setTimeout(() => {
    $$(".lang-img").forEach((img) => { new Image().src = img.dataset.srcZh; new Image().src = img.dataset.srcEn; });
  }, 1500));

  // Copy buttons.
  $$("[data-copy]").forEach((b) => {
    b.addEventListener("click", async () => {
      try { await navigator.clipboard.writeText(b.dataset.copy); } catch (e) { return; }
      const label = b.querySelector(".copy-label");
      const before = label.innerHTML;
      label.innerHTML = '<span class="text-coral">' + t("已复制", "Copied") + "</span>";
      setTimeout(() => { label.innerHTML = before; }, 1600);
    });
  });

  // Rise into view.
  const io = new IntersectionObserver((entries) => {
    entries.forEach((e) => { if (e.isIntersecting) { e.target.classList.add("in"); io.unobserve(e.target); } });
  }, { threshold: 0.12 });
  $$(".reveal").forEach((el) => io.observe(el));

  // Nav: the section you're in is marked.
  const navLinks = $$(".nav-link");
  const navIO = new IntersectionObserver((entries) => {
    entries.forEach((e) => {
      if (!e.isIntersecting) return;
      navLinks.forEach((a) => {
        const on = a.dataset.nav === e.target.id;
        a.classList.toggle("text-fg", on);
        a.classList.toggle("bg-fg/[.06]", on);
      });
    });
  }, { rootMargin: "-45% 0px -50% 0px" });
  ["top", "tour", "mo", "terminal", "install"].forEach((id) => { const el = document.getElementById(id); if (el) navIO.observe(el); });

  // The tour: scrolling picks the step; tabs and arrows scroll to one.
  const steps = $$(".step");
  const screens = $$(".screen");
  const tabs = $$(".tour-tab");
  const prev = document.querySelector(".tour-prev");
  const next = document.querySelector(".tour-next");
  let current = 0;
  function showStep(i) {
    current = i;
    const id = steps[i].dataset.step;
    steps.forEach((s, j) => s.classList.toggle("on", j === i));
    screens.forEach((s) => s.classList.toggle("on", s.dataset.screen === id));
    choose(tabs, (b) => b.dataset.go === id);
    // Only a strip that overflows (phones) needs to bring its tab into view; a
    // needless smooth scroll here would cancel the page's own (e.g. a #link jump).
    const tab = tabs.find((b) => b.dataset.go === id);
    const strip = tab && tab.parentElement;
    if (strip && strip.scrollWidth > strip.clientWidth) strip.scrollTo({ left: tab.offsetLeft - 40, behavior: calm ? "auto" : "smooth" });
    prev.disabled = i === 0;
    next.disabled = i === steps.length - 1;
  }
  function goTo(i) {
    const step = steps[Math.max(0, Math.min(steps.length - 1, i))];
    // Land the step where the observer reads it: mid-screen on desktop, below the stage on phones.
    const wide = matchMedia("(min-width: 1024px)").matches;
    const box = step.getBoundingClientRect();
    const target = wide ? innerHeight / 2 : innerHeight * 0.78;
    scrollBy({ top: box.top + box.height / 2 - target, behavior: calm ? "auto" : "smooth" });
  }
  tabs.forEach((b) => b.addEventListener("click", () => goTo(steps.findIndex((s) => s.dataset.step === b.dataset.go))));
  prev.addEventListener("click", () => goTo(current - 1));
  next.addEventListener("click", () => goTo(current + 1));
  let stepIO = null;
  function watchSteps() {
    if (stepIO) stepIO.disconnect();
    const wide = matchMedia("(min-width: 1024px)").matches;
    stepIO = new IntersectionObserver((entries) => {
      entries.forEach((e) => { if (e.isIntersecting) showStep(steps.indexOf(e.target)); });
    }, { rootMargin: wide ? "-49% 0px -49% 0px" : "-77% 0px -21% 0px" });
    steps.forEach((s) => stepIO.observe(s));
  }
  matchMedia("(min-width: 1024px)").addEventListener("change", watchSteps);
  watchSteps();
  showStep(0);

  // Keycaps: ⌃ then ⌃, and the panel answers.
  const caps = $$("#keys .keycap");
  const summon = document.querySelector("#keys .summon");
  if (!calm && caps.length === 2) {
    const press = () => {
      setTimeout(() => caps[0].classList.add("down"), 0);
      setTimeout(() => caps[0].classList.remove("down"), 150);
      setTimeout(() => caps[1].classList.add("down"), 300);
      setTimeout(() => caps[1].classList.remove("down"), 450);
      setTimeout(() => summon && summon.classList.add("on"), 520);
      setTimeout(() => summon && summon.classList.remove("on"), 2600);
    };
    press();
    setInterval(press, 3400);
  } else if (summon) {
    summon.classList.add("on");
  }

  // "Bring all back": the rows come back one by one.
  const restoreBtn = document.querySelector(".restore-demo");
  const rows = $$("#restore li");
  function restore() {
    if (restoreBtn.disabled) return;
    restoreBtn.disabled = true;
    rows.forEach((row, i) => {
      setTimeout(() => {
        row.querySelector(".dot").style.background = "#ff5a3c";
        const state = row.querySelector(".row-state");
        state.dataset.before = state.dataset.before || state.innerHTML;
        state.innerHTML = '<span class="text-coral">' + t("回来了", "back") + "</span>";
      }, 350 + i * 380);
    });
    setTimeout(() => {
      rows.forEach((row) => {
        row.querySelector(".dot").style.background = "";
        const state = row.querySelector(".row-state");
        state.innerHTML = state.dataset.before;
      });
      restoreBtn.disabled = false;
    }, 4600);
  }
  if (restoreBtn) {
    restoreBtn.addEventListener("click", restore);
    // Show it once on its own when it comes into view.
    const rio = new IntersectionObserver((e) => {
      if (e[0].isIntersecting) { setTimeout(restore, 600); rio.disconnect(); }
    }, { threshold: 0.8 });
    rio.observe(restoreBtn);
  }

  // Mo: three tempers, and a poke.
  const bubble = document.getElementById("mo-bubble");
  const modeCaption = document.getElementById("mode-caption");
  const modes = {
    quiet: { zh: ["只做个表情，你点它才说。", "…"], en: ["Just a look. Click Mo to hear more.", "…"] },
    everyday: { zh: ["先冒个泡，你凑过来再细说。", "有件事～"], en: ["A small hint first. The rest when you come closer.", "Got a sec?"] },
    active: { zh: ["直接把消息框弹给你，按钮都备好了。", "接口评审在等你点头"], en: ["The full message, buttons included.", "api-review needs your OK."] },
  };
  let mode = "everyday";
  const pokes = {
    zh: ["嗨，我是小莫", "别戳啦，痒～", "账单修复断了，点我就能找回", "找回来啦，从刚才的地方接着来", "官网改版正在忙，我盯着呢"],
    en: ["Hi, I'm Mo.", "Hey, that tickles.", "billing-fix stopped. Want it back?", "Back where you left off.", "acme-web is busy. I've got it."],
  };
  let poke = 0;
  function say(text) {
    if (!bubble) return;
    bubble.classList.remove("pop");
    void bubble.offsetWidth;
    bubble.textContent = text;
    bubble.classList.add("pop");
  }
  const modeButtons = $$(".mode");
  modeButtons.forEach((b) => b.addEventListener("click", () => { mode = b.dataset.mode; refreshMo(); say(modes[mode][lang()][1]); }));
  const pokeBtn = document.getElementById("mo-poke");
  if (pokeBtn) pokeBtn.addEventListener("click", () => { say(pokes[lang()][poke++ % pokes[lang()].length]); });
  function refreshMo() {
    choose(modeButtons, (b) => b.dataset.mode === mode);
    if (modeCaption) modeCaption.textContent = modes[mode][lang()][0];
    if (bubble) say(modes[mode][lang()][1]);
  }

  // The terminal types `t`, and the picker appears.
  const typed = document.getElementById("typed");
  const picker = document.getElementById("picker");
  if (typed && picker) {
    const run = () => {
      typed.textContent = "";
      picker.classList.add("hidden");
      setTimeout(() => { typed.textContent = "t"; }, 700);
      setTimeout(() => { picker.classList.remove("hidden"); }, 1200);
    };
    const tio = new IntersectionObserver((e) => {
      if (e[0].isIntersecting) { run(); if (!calm) setInterval(run, 6000); tio.disconnect(); }
    });
    tio.observe(typed);
  }

  setLang(lang());
})();
