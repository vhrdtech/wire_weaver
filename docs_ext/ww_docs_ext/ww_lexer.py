"""The `ww` fence: a transcript of the `ww` command line tool, coloured as a reader scans it.

A transcript is command lines after a `$ ` prompt and the output that follows each. The command's words are marked
by their place, as the `console` fence's lexer marks them: the command (`Name.Function`), the subcommands before the
first flag (`Name.Tag`), the flags (`Name.Attribute`), a trailing `# comment`.

Output lines are of two kinds:

- An API tree, as `ww introspect` and `ww api tree` print it: a line that starts with `trait `, `types` or a tree
  branch (`├─`, `└─`, `│`). Its keywords (`fn`, `rw`, `property`, `stream`, `impl`, ...), the item's name after
  them, its number, attributes (`#[effect(edit)]`), types and bounds each get their own colour.
- Any other line: an error or warning label at its start, a mark (`✔`, `✗`), and in the rest numbers, versions,
  `crate@version`, hashes, strings and Rust paths. Plain words stay plain, so a sentence reads as a sentence.

Wired up through the package's `pygments.lexers` entry point (pyproject.toml), as alias `ww`; a page writes the
generated transcript into a ```` ```ww ```` fence. Colours of the tokens the code colours leave plain (errors,
marks, subcommands) are in docs/stylesheets/vhrd.css (`.language-ww`).
"""

import re

from pygments.lexer import Lexer
from pygments.token import Comment, Generic, Keyword, Name, Number, Operator, Punctuation, String, Text

PROMPT = "$ "
SUBCOMMAND = re.compile(r"[a-z][a-z0-9-]*\Z")
# a word of a command line: a quoted string or a run without spaces
WORD = re.compile(r"""\s+|#.*|'[^']*'?|"(?:[^"\\]|\\.)*"?|[^\s'"]+""")

TREE_START = re.compile(r"\s*(?:trait |types\b|[│├└])")
TREE_PREFIX = re.compile(r"\s*[│├└][│├└─ ]*")
# a label at the start of an output line: an error, a warning or a note, then the rest of the line
LABEL = re.compile(r"(?:(Error|error)|(warning|Warning)|(note))(?=[:\s\[])")
MARKS = {"✔": Generic.Inserted, "✓": Generic.Inserted, "✗": Generic.Deleted, "?": Generic.Emph}

_COMMON = [
    (r'"(?:[^"\\]|\\.)*"', String),
    (r"(?:0x[0-9a-fA-F]+|(?=[0-9a-f]*[0-9])(?=[0-9a-f]*[a-f])[0-9a-f]{8,})\b", Number.Hex),
    (r"\bw\d+\.\d+\b", Number),
    (r"\b\d+\.\d+\.\d+(?:-[\w.]+)?\b", Number),
    (r"\b(?:true|false|Some|None|Ok|Err)\b", Keyword.Constant),
    (r"(?<![\w.])-?\d+(?:\.\d+)?(?:e-?\d+)?(?!\d|\.\d)", Number),
]
# `crate@1.2.3`: the crate, the `@` and the version
_CRATE_VERSION = re.compile(r"([a-z_][a-z0-9_]*)(@)(\d+\.\d+\.\d+(?:-[\w.]+)?)")

_TREE = [
    (r"#\[[^\[\]]*\]", Name.Decorator),
    (r"\b(fn|property|stream|sink|impl)(\s+)([A-Za-z_]\w*)", None),
    (r"\b(?:trait|types|rw|ro|wo|observable|enum|struct|sized|repr|write error|read error)\b", Keyword),
    (
        r"\b(?:[ui]\d+|u[bn]\d+|ib\d+|[ui]vlq\d+|f32|f64|bool|nib|str|String|Vec|RefVec)\b",
        Keyword.Type,
    ),
    (r"\b(?:[A-Za-z_]\w*::)+[A-Za-z_]\w*", Name.Class),
    (r"\b[A-Z]\w*", Name.Class),
    *_COMMON,
    (r"->|\.\.=|\.\.", Operator),
    (r"[()\[\]<>{},:;=]", Punctuation),
]
_PLAIN = [
    *_COMMON,
    (r"\b(?:[A-Za-z_]\w*::)+[A-Za-z_]\w*", Name.Class),
    (r"->|\.\.=", Operator),
]


def _compile(rules):
    """One pattern of all the rules, each its own group, and per rule its group's number and its token"""
    groups, number = [], 1
    for pattern, kind in rules:
        groups.append((number, kind))
        number += 1 + re.compile(pattern).groups
    return re.compile("|".join(f"({pattern})" for pattern, _ in rules)), groups


_TREE_RE = _compile(_TREE)
_PLAIN_RE = _compile(_PLAIN)


class WwLexer(Lexer):
    name = "WireWeaver ww transcript"
    aliases = ["ww"]
    filenames = []
    mimetypes = []

    def get_tokens_unprocessed(self, text):
        pos = 0
        for line in text.splitlines(keepends=True):
            if line.startswith(PROMPT):
                yield pos, Generic.Prompt, PROMPT
                yield from _command(line[len(PROMPT) :], pos + len(PROMPT))
            else:
                yield from _output(line, pos)
            pos += len(line)


def _command(line, start):
    """A command line's tokens, its words marked by their place"""
    # a flag without `=` may take the next word as its value; subcommands go on after it (`ww --websocket URL list`)
    at_command, in_subcommands, flag_value = True, False, False
    for m in WORD.finditer(line):
        word, at = m.group(0), start + m.start()
        if word.isspace():
            yield at, Text, word
        elif word.startswith("#"):
            yield at, Comment.Single, word
        elif word[0] in "'\"":
            in_subcommands = flag_value = False
            yield at, String, word
        elif word.startswith("-"):
            flag_value = "=" not in word
            yield at, Name.Attribute, word
        elif flag_value:
            flag_value = False
            yield at, Text, word
        elif at_command:
            at_command, in_subcommands = False, True
            yield at, Name.Function, word
        elif in_subcommands and SUBCOMMAND.match(word):
            yield at, Name.Tag, word
        else:
            in_subcommands = False
            yield at, Text, word


def _output(line, start):
    """An output line's tokens: an API tree line, or any other"""
    i = 0
    if TREE_START.match(line):
        prefix = TREE_PREFIX.match(line)
        if prefix:
            yield start, Punctuation, prefix.group(0)
            i = prefix.end()
        # the item's number after a branch
        number = re.compile(r"\d+(?= )").match(line, i)
        if number and prefix:
            yield start + i, Number.Integer, number.group(0)
            i = number.end()
        yield from _scan(line, i, start, _TREE_RE)
        return
    label = LABEL.match(line)
    if label:
        kind = Generic.Error if label.group(1) else Generic.Strong if label.group(2) else Generic.Emph
        yield start, kind, label.group(0)
        i = label.end()
    else:
        mark = MARKS.get(line[:1])
        if mark is not None:
            yield start, mark, line[:1]
            i = 1
    yield from _scan(line, i, start, _PLAIN_RE)


def _scan(line, i, start, compiled):
    """The tokens of `line[i:]` by the first rule that matches at each place; text between matches is plain"""
    pattern, groups = compiled
    plain_from = i
    while i < len(line):
        crate = _CRATE_VERSION.match(line, i)
        if crate and (i == 0 or not (line[i - 1].isalnum() or line[i - 1] == "_")):
            if plain_from < i:
                yield start + plain_from, Text, line[plain_from:i]
            yield start + i, Name.Namespace, crate.group(1)
            yield start + crate.start(2), Punctuation, "@"
            yield start + crate.start(3), Number, crate.group(3)
            i = plain_from = crate.end()
            continue
        m = pattern.match(line, i)
        if not m or m.end() == i:
            # skip to the next word or sign so a rule anchored on `\b` can match at its start
            i += 1
            while i < len(line) and line[i - 1].isalnum() and line[i].isalnum():
                i += 1
            continue
        if plain_from < i:
            yield start + plain_from, Text, line[plain_from:i]
        group, kind = next((g, k) for g, k in groups if m.group(g) is not None)
        if kind is None:
            # keyword, space, name
            yield start + m.start(group + 1), Keyword, m.group(group + 1)
            yield start + m.start(group + 2), Text, m.group(group + 2)
            yield start + m.start(group + 3), Name.Function, m.group(group + 3)
        else:
            yield start + i, kind, m.group(0)
        i = plain_from = m.end()
    if plain_from < len(line):
        yield start + plain_from, Text, line[plain_from:]
