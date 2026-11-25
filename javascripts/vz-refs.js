// vz: opens a reference's card (vz-refs.css) beside it, written by `vz` (do not edit). A
// reference is a link naming its card in `data-vz-card`; without this script it stays a plain link. A card opens on
// hover, on keyboard focus and on a first tap (a second tap follows the link), and closes on Escape, on a tap
// elsewhere and when the pointer or focus leaves both reference and card. The cards sit at the end of the page, so
// Tab from an open card's reference goes into its links, and Tab past them (Shift+Tab before them) goes on from the
// reference. No card opens while the reader selects text: a mouse button held down, or a selection that holds the
// reference; the card of a reference the pointer rests on opens when the selection is cleared. Listeners sit on the
// document, so pages that instant navigation loads later need nothing.
(function () {
  var open = null;
  var timer = null;
  var touch = false;
  var pressed = false;
  var over = null;

  function refOf(node) {
    return node && node.closest ? node.closest("a.vz-ref[data-vz-card]") : null;
  }
  function cardOf(ref) {
    return document.getElementById(ref.getAttribute("data-vz-card"));
  }
  function place(ref, card) {
    var r = ref.getBoundingClientRect();
    var room = document.documentElement.clientWidth;
    card.style.left = "0px";
    card.style.top = "0px";
    var w = card.offsetWidth;
    var h = card.offsetHeight;
    var left = Math.max(8, Math.min(r.left, room - w - 8));
    var top = r.bottom + 4;
    if (top + h > window.innerHeight - 8 && r.top - h - 4 > 8) top = r.top - h - 4;
    card.style.left = Math.round(left) + "px";
    card.style.top = Math.round(top) + "px";
  }
  // whether a non-empty selection holds `ref`, wholly or in part
  function selected(ref) {
    var sel = window.getSelection ? window.getSelection() : null;
    return !!sel && !sel.isCollapsed && sel.rangeCount > 0 && sel.containsNode(ref, true);
  }
  // whether the reader is selecting at `ref`: a mouse drag in progress, or a selection holding it
  function selecting(ref) {
    return pressed || selected(ref);
  }
  function show(ref) {
    var card = cardOf(ref);
    if (!card) return;
    clearTimeout(timer);
    if (open && open.card !== card) hide();
    card.classList.add("vz-open");
    ref.setAttribute("aria-expanded", "true");
    place(ref, card);
    open = { ref: ref, card: card };
  }
  function hide() {
    clearTimeout(timer);
    if (!open) return;
    open.card.classList.remove("vz-open");
    open.ref.setAttribute("aria-expanded", "false");
    open = null;
  }
  function later() {
    clearTimeout(timer);
    timer = setTimeout(hide, 300);
  }
  function inOpen(node) {
    return open && node && (open.ref.contains(node) || open.card.contains(node));
  }
  function links(root) {
    return Array.prototype.slice.call(root.querySelectorAll("a[href]"));
  }
  // the focusable element next to `ref` in the page, leaving out the cards
  function beside(ref, step) {
    var all = Array.prototype.slice
      .call(document.querySelectorAll("a[href], button, input, select, textarea, [tabindex]"))
      .filter(function (el) {
        return el.tabIndex >= 0 && !el.disabled && !el.closest(".vz-cards") && el.getClientRects().length;
      });
    return all[all.indexOf(ref) + step] || null;
  }
  function tab(e) {
    var inside = links(open.card);
    var at = inside.indexOf(document.activeElement);
    var next = null;
    if (document.activeElement === open.ref && !e.shiftKey) next = inside[0];
    else if (at === 0 && e.shiftKey) next = open.ref;
    else if (at === inside.length - 1 && !e.shiftKey) next = beside(open.ref, 1);
    if (!next) return;
    e.preventDefault();
    next.focus();
  }

  document.addEventListener("pointerdown", function (e) {
    touch = e.pointerType === "touch" || e.pointerType === "pen";
    pressed = !touch && e.button === 0;
    if (open && !inOpen(e.target)) hide();
  });
  function released() {
    pressed = false;
  }
  document.addEventListener("pointerup", released);
  document.addEventListener("pointercancel", released);
  document.addEventListener("click", function (e) {
    var ref = refOf(e.target);
    if (!ref || !touch || selecting(ref)) return;
    if (!open || open.ref !== ref) {
      e.preventDefault();
      show(ref);
    }
  });
  document.addEventListener("mouseover", function (e) {
    if (touch) return;
    if (!e.buttons) pressed = false;
    var ref = refOf(e.target);
    over = ref;
    if (ref) {
      if (!selecting(ref)) show(ref);
    } else if (inOpen(e.target)) clearTimeout(timer);
  });
  document.addEventListener("mouseout", function (e) {
    if (!refOf(e.relatedTarget)) over = null;
    if (!touch && inOpen(e.target) && !inOpen(e.relatedTarget)) later();
  });
  // a selection that grows over the open reference closes its card; a cleared one lets the reference under the
  // pointer open its card
  document.addEventListener("selectionchange", function () {
    if (open && selected(open.ref)) hide();
    else if (!open && over && !selecting(over)) show(over);
  });
  document.addEventListener("focusin", function (e) {
    var ref = refOf(e.target);
    if (ref) {
      if (!selecting(ref)) show(ref);
    }
    else if (inOpen(e.target)) clearTimeout(timer);
  });
  document.addEventListener("focusout", function (e) {
    if (inOpen(e.target) && !inOpen(e.relatedTarget)) later();
  });
  document.addEventListener("keydown", function (e) {
    if (!open) return;
    if (e.key === "Tab") tab(e);
    else if (e.key === "Escape") {
      if (open.card.contains(document.activeElement)) open.ref.focus();
      hide();
    }
  });
  window.addEventListener("scroll", function () {
    if (open) place(open.ref, open.card);
  }, { passive: true });
  window.addEventListener("resize", function () {
    if (open) place(open.ref, open.card);
  });
})();
