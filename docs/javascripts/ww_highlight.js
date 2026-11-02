// WireWeaver-specific syntax highlighting on top of the already-highlighted ```rust blocks
// (Pygments, run by pymdownx.highlight at build time). Adds classes to tokens Pygments'
// generic Rust lexer has no idea about: property!/method!/stream! and their ro/rw/wo/observe
// access modes, WireWeaver attributes, and exact_units' unit/prefix/wrapper names. Colours are
// in stylesheets/vhrd.css, reusing the theme's --md-code-hl-*-color variables.
//
// The token lists live in ww_tokens.json next to this file, kept separate so ww_lsp and an
// editor grammar can reuse them without parsing JS.
(() => {
  const SCRIPT_URL = document.currentScript && document.currentScript.src;
  const DATA_URL = SCRIPT_URL ? new URL("ww_tokens.json", SCRIPT_URL) : "ww_tokens.json";

  const DECL_KEYWORDS = new Set([
    "struct", "enum", "trait", "fn", "type", "mod", "const", "static", "impl", "union",
  ]);

  let tokensPromise = null;
  function loadTokens() {
    if (!tokensPromise) {
      tokensPromise = fetch(DATA_URL)
        .then((r) => r.json())
        .then((data) => ({
          macros: new Set(data.macros),
          accessModes: new Set(data.accessModes),
          attributes: new Set(data.attributes),
          wrappers: new Set(data.wrappers),
          prefixes: new Set(data.prefixes),
          units: new Set(data.units),
          origins: new Set(data.origins),
          aliases: new Set(data.aliases),
        }));
    }
    return tokensPromise;
  }

  // toks: flat array of token <span> elements for one highlighted line, in source order.
  function markMacroCall(toks, i, T) {
    const tok = toks[i];
    const bang = toks[i + 1];
    if (!bang || bang.className !== "o" || bang.textContent !== "!") return;
    tok.classList.add("ww-tok", "ww-macro");

    let j = i + 2;
    while (toks[j] && toks[j].className === "w") j++;
    if (!(toks[j] && toks[j].className === "p" && toks[j].textContent === "(")) return;
    j++;
    while (toks[j] && toks[j].className === "w") j++;
    while (
      toks[j] &&
      ((toks[j].className === "n" && T.accessModes.has(toks[j].textContent)) ||
        (toks[j].className === "o" && toks[j].textContent === "+"))
    ) {
      if (toks[j].className === "n") toks[j].classList.add("ww-tok", "ww-mode");
      j++;
    }
  }

  function markAttribute(tok, T) {
    const m = /^#!?\[\s*([A-Za-z_][A-Za-z0-9_]*)/.exec(tok.textContent);
    if (m && T.attributes.has(m[1])) tok.classList.add("ww-tok", "ww-attr");
  }

  function markExactUnits(toks, i, T) {
    const tok = toks[i];
    const text = tok.textContent;
    if (!/^[A-Z]/.test(text)) return;

    let p = i - 1;
    while (p >= 0 && toks[p].className === "w") p--;
    const prev = p >= 0 ? toks[p] : null;
    if (prev && (prev.className === "k" || prev.className === "kd") && DECL_KEYWORDS.has(prev.textContent)) {
      return; // a type's own name at its definition, not a use of an exact_units name
    }
    if (prev && prev.className === "p" && prev.textContent === "::") {
      let q = p - 1;
      while (q >= 0 && toks[q].className === "w") q--;
      const qualifier = q >= 0 ? toks[q] : null;
      if (qualifier && (qualifier.className === "n" || qualifier.className === "nc") && /^[A-Z]/.test(qualifier.textContent)) {
        return; // Type::Name, e.g. an enum variant on an unrelated type that happens to share the word
      }
    }

    if (T.units.has(text) || T.origins.has(text) || T.aliases.has(text)) {
      tok.classList.add("ww-tok", "ww-unit");
    } else if (T.prefixes.has(text)) {
      tok.classList.add("ww-tok", "ww-prefix");
    } else if (T.wrappers.has(text)) {
      tok.classList.add("ww-tok", "ww-wrapper");
    }
  }

  function highlightBlock(codeEl, T) {
    if (codeEl.dataset.wwHighlighted) return;
    codeEl.dataset.wwHighlighted = "1";

    for (const lineSpan of codeEl.children) {
      if (lineSpan.tagName !== "SPAN") continue;
      const toks = Array.from(lineSpan.children).filter((el) => el.tagName === "SPAN");

      for (let i = 0; i < toks.length; i++) {
        const tok = toks[i];
        const cls = tok.className;
        const text = tok.textContent;

        if (cls === "n" && T.macros.has(text)) {
          markMacroCall(toks, i, T);
        } else if (cls === "cp" && text.startsWith("#")) {
          markAttribute(tok, T);
        } else if (cls === "n" || cls === "nc") {
          markExactUnits(toks, i, T);
        }
      }
    }
  }

  document$.subscribe(({ body }) => {
    const blocks = body.querySelectorAll(".language-rust.highlight code");
    if (blocks.length === 0) return;
    loadTokens().then((T) => blocks.forEach((code) => highlightBlock(code, T)));
  });
})();
