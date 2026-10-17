// The landing page's small bits of life. No framework: a handful of
// listeners, and every animation stops when the visitor prefers less motion.
(() => {
  const root = document.documentElement;
  const calm = matchMedia("(prefers-reduced-motion: reduce)").matches;
  const lang = () => root.dataset.lang;
  const t = (zh, en) => (lang() === "zh" ? zh : en);

  // Language: every phrase is in the page twice; pictures swap their source.
  function setLang(next) {
    root.dataset.lang = next;
    root.lang = next === "zh" ? "zh-CN" : "en";
    try { localStorage.setItem("lang", next); } catch (e) {}
    document.querySelectorAll(".lang-img").forEach((img) => {
      const src = img.dataset["src" + (next === "zh" ? "Zh" : "En")];
      if (src && img.getAttribute("src") !== src) img.src = src;
    });
    document.querySelectorAll(".lang-btn").forEach((b) => {
      const on = b.dataset.setLang === next;
      b.classList.toggle("bg-white/15", on);
      b.classList.toggle("text-white", on);
      b.classList.toggle("text-white/50", !on);
    });
    document.title = t("cc-moss · 文明会延续，对话也是。", "cc-moss · Civilizations carry on. So do conversations.");
    refreshCaptions();
  }
  document.querySelectorAll("[data-set-lang]").forEach((b) => b.addEventListener("click", () => setLang(b.dataset.setLang)));

  // Copy buttons.
  document.querySelectorAll("[data-copy]").forEach((b) => {
    b.addEventListener("click", async () => {
      try { await navigator.clipboard.writeText(b.dataset.copy); } catch (e) { return; }
      const label = b.querySelector(".copy-label");
      const before = label.innerHTML;
      label.innerHTML = '<span class="text-mint">' + t("已复制", "Copied") + "</span>";
      setTimeout(() => { label.innerHTML = before; }, 1600);
    });
  });

  // Rise into view.
  const io = new IntersectionObserver((entries) => {
    entries.forEach((e) => { if (e.isIntersecting) { e.target.classList.add("in"); io.unobserve(e.target); } });
  }, { threshold: 0.15 });
  document.querySelectorAll(".reveal").forEach((el) => io.observe(el));

  // Showcase: the dock picks a shot; it also turns by itself until you touch it.
  const docks = [...document.querySelectorAll(".dock")];
  const shots = [...document.querySelectorAll(".shot")];
  const caption = document.getElementById("shot-caption");
  let current = "panel";
  let auto = null;
  docks.forEach((d) => {
    d.className = "dock w-12 h-12 rounded-xl grid place-items-center text-white/60 hover:text-white hover:bg-white/10 transition cursor-pointer [&>svg]:w-5 [&>svg]:h-5";
    d.addEventListener("click", () => { stopAuto(); show(d.dataset.dock); });
  });
  function show(id) {
    current = id;
    shots.forEach((s) => { s.hidden = s.dataset.shot !== id; });
    docks.forEach((d) => {
      const on = d.dataset.dock === id;
      d.classList.toggle("bg-white/15", on);
      d.classList.toggle("text-white", on);
      d.classList.toggle("shadow-[inset_0_1px_0_rgba(255,255,255,.15)]", on);
      d.setAttribute("aria-selected", on);
    });
    refreshCaptions();
  }
  function stopAuto() { if (auto) { clearInterval(auto); auto = null; } }
  if (!calm) {
    auto = setInterval(() => {
      const i = docks.findIndex((d) => d.dataset.dock === current);
      show(docks[(i + 1) % docks.length].dataset.dock);
    }, 5200);
  }

  // Keycaps: ⌃ then ⌃, again and again.
  const caps = [...document.querySelectorAll("#keys .keycap")];
  if (!calm && caps.length === 2) {
    setInterval(() => {
      setTimeout(() => caps[0].classList.add("down"), 0);
      setTimeout(() => caps[0].classList.remove("down"), 160);
      setTimeout(() => caps[1].classList.add("down"), 330);
      setTimeout(() => caps[1].classList.remove("down"), 490);
    }, 2400);
  }

  // "Bring all back": a little demo.
  document.querySelectorAll(".restore-demo").forEach((b) => {
    b.addEventListener("click", () => {
      const before = b.innerHTML;
      b.disabled = true;
      b.innerHTML = t("正在找回…", "Bringing back…");
      setTimeout(() => { b.innerHTML = "✓ " + t("都回来了", "All back"); }, 1300);
      setTimeout(() => { b.innerHTML = before; b.disabled = false; }, 3400);
    });
  });

  // Mo: three tempers, and a poke.
  const bubble = document.getElementById("mo-bubble");
  const modeCaption = document.getElementById("mode-caption");
  const modes = {
    quiet: { zh: ["只做个表情，你点它才说。", "…"], en: ["Just a face. Click Mo to hear more.", "…"] },
    everyday: { zh: ["先冒个泡，你凑过来再细说。", "有件事～"], en: ["A little puff first; the rest when you come over.", "Got a sec?"] },
    active: { zh: ["直接把消息框弹给你，按钮都备好了。", "接口评审在等你点头！"], en: ["Pops the message right up, buttons and all.", "api-review is waiting for your OK!"] },
  };
  let mode = "everyday";
  const pokes = {
    zh: ["嗨，我是小莫！", "别戳啦，痒～", "账单修复断了…点我就能找回", "找回来啦！从刚才的地方接着来。", "官网改版正在忙，我盯着呢"],
    en: ["Hi! I'm Mo.", "Hey, that tickles!", "billing-fix stopped… click me to bring it back", "It's back! Right where you left off.", "acme-web is busy; I'm watching"],
  };
  let poke = 0;
  function say(text) {
    if (!bubble) return;
    bubble.classList.remove("pop");
    void bubble.offsetWidth;
    bubble.textContent = text;
    bubble.classList.add("pop");
  }
  document.querySelectorAll(".mode").forEach((b) => b.addEventListener("click", () => { mode = b.dataset.mode; refreshCaptions(); say(modes[mode][lang()][1]); }));
  const pokeBtn = document.getElementById("mo-poke");
  if (pokeBtn) pokeBtn.addEventListener("click", () => { say(pokes[lang()][poke++ % pokes[lang()].length]); });

  function refreshCaptions() {
    const d = docks.find((x) => x.dataset.dock === current);
    if (caption && d) caption.textContent = d.dataset[lang()];
    document.querySelectorAll(".mode").forEach((b) => {
      const on = b.dataset.mode === mode;
      b.classList.toggle("bg-mint/20", on);
      b.classList.toggle("text-mint", on);
      b.classList.toggle("bg-white/5", !on);
    });
    if (modeCaption) modeCaption.textContent = modes[mode][lang()][0];
    if (bubble && !bubble.textContent) say(modes[mode][lang()][1]);
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
  show("panel");
})();
