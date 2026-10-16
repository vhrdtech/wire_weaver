---
icon: lucide/signal
level: basic
---

# Reading levels

Pages carry a small badge above their title: how deep the page goes. Read the Basic pages first and stop when you
have what you need. The deeper levels are there for when you hit a limit, need the last bit of size or speed, or
want to know why something works the way it does.

<div class="grid cards ww-level-cards" markdown>

-   <span class="ww-level ww-level--basic"><svg class="ww-level__icon" viewBox="0 0 14 12" aria-hidden="true"><rect x="0" y="7" width="3.2" height="5" rx="0.6"/><rect x="5.4" y="3.5" width="3.2" height="8.5" rx="0.6" class="ww-level__off"/><rect x="10.8" y="0" width="3.2" height="12" rx="0.6" class="ww-level__off"/></svg><span class="ww-level__label">Basic</span></span>

    ---
    What you need to use WireWeaver: what a feature is for, the common case, a working example. Enough to define
    types, put them on the wire and read them back. Start here.

-   <span class="ww-level ww-level--advanced"><svg class="ww-level__icon" viewBox="0 0 14 12" aria-hidden="true"><rect x="0" y="7" width="3.2" height="5" rx="0.6"/><rect x="5.4" y="3.5" width="3.2" height="8.5" rx="0.6"/><rect x="10.8" y="0" width="3.2" height="12" rx="0.6" class="ww-level__off"/></svg><span class="ww-level__label">Advanced</span></span>

    ---
    Every option and when to pick it: compact layouts, special types, the full list of macro directives, worked
    byte-level examples. For when the defaults are not enough. Assumes the Basic pages.

-   <span class="ww-level ww-level--nomicon"><svg class="ww-level__icon" viewBox="0 0 14 12" aria-hidden="true"><rect x="0" y="7" width="3.2" height="5" rx="0.6"/><rect x="5.4" y="3.5" width="3.2" height="8.5" rx="0.6"/><rect x="10.8" y="0" width="3.2" height="12" rx="0.6"/></svg><span class="ww-level__label">Nomicon</span></span>

    ---
    The internals, as in the Rustonomicon: exact bit layouts, how the reader and writer work inside, measurements
    and the reasons behind design decisions. For writing your own serializer, a reader in another language, or
    changing WireWeaver itself.

</div>

A level says how much a page assumes, not how important it is: a Nomicon page can be the one you need for a
bootloader that has to fit in 2 KB.

!!! note "For writers"
    The badge comes from the page's front matter, `level: basic`, `level: advanced` or `level: nomicon`, and is
    drawn by `overrides/partials/level.html`. A page without it has no badge. In the navigation, a section lists its
    Basic pages first and groups the Advanced and Nomicon pages after them.
