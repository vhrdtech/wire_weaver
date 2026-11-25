// vz-stamp-age: written by `vz stamp`, turns each .book-stamp[data-built]'s build time into "built N ago",
// coarser as it grows (seconds, minutes, hours, days), beside the absolute UTC time already in the stamp.
(function () {
  function render(el) {
    var span = el.querySelector(".book-stamp-age");
    var built = Number.parseInt(el.dataset.built, 10) * 1000;
    if (!span || Number.isNaN(built)) {
      return;
    }
    var s = Math.max(0, Math.round((Date.now() - built) / 1000));
    var text;
    if (s < 60) {
      text = s + " s ago";
    } else if (s < 3600) {
      text = Math.floor(s / 60) + " min ago";
    } else if (s < 86400) {
      text = Math.floor(s / 3600) + " h ago";
    } else {
      text = Math.floor(s / 86400) + " days ago";
    }
    span.textContent = text;
  }
  function tick() {
    document.querySelectorAll(".book-stamp[data-built]").forEach(render);
  }
  tick();
  setInterval(tick, 30000);
})();
