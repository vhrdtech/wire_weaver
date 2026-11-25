"""The `ww` fence's lexer (ww_docs_ext.ww_lexer)."""

from pathlib import Path

from pygments.lexers import get_lexer_by_name
from pygments.token import Comment, Generic, Keyword, Name, Number, Punctuation, Text

from ww_docs_ext.ww_lexer import WwLexer

ROOT = Path(__file__).resolve().parents[2]


def tokens(text):
    """The tokens of `text` that are not plain text, as (kind, value)"""
    out = list(WwLexer().get_tokens_unprocessed(text))
    assert "".join(value for _, _, value in out) == text, "the tokens cover the text exactly"
    return [(kind, value) for _, kind, value in out if kind is not Text]


def test_pygments_finds_the_lexer_by_its_alias():
    assert isinstance(get_lexer_by_name("ww"), WwLexer)


def test_a_command_line_marks_the_command_its_subcommands_and_flags():
    assert tokens("$ ww --websocket ws://127.0.0.1:8080/ww introspect -d   # no docs\n") == [
        (Generic.Prompt, "$ "),
        (Name.Function, "ww"),
        (Name.Attribute, "--websocket"),
        (Name.Tag, "introspect"),
        (Name.Attribute, "-d"),
        (Comment.Single, "# no docs"),
    ]


def test_an_api_tree_line_marks_number_attribute_keywords_name_and_type():
    line = "│  ├─ 3 #[effect(edit)] rw property gain: Fixed<Ratio, One, u8, Range<1, 16>> [1..=16] observable\n"
    got = tokens(line)
    assert got[:6] == [
        (Punctuation, "│  ├─ "),
        (Number.Integer, "3"),
        (Name.Decorator, "#[effect(edit)]"),
        (Keyword, "rw"),
        (Keyword, "property"),
        (Name.Function, "gain"),
    ]
    assert (Keyword.Type, "u8") in got
    assert (Name.Class, "Ratio") in got
    assert (Number, "16") in got
    assert got[-1] == (Keyword, "observable")


def test_a_tree_root_marks_the_crate_and_its_version():
    assert tokens("trait Thermo thermometer_api@1.0.0\n") == [
        (Keyword, "trait"),
        (Name.Class, "Thermo"),
        (Name.Namespace, "thermometer_api"),
        (Punctuation, "@"),
        (Number, "1.0.0"),
    ]


def test_other_output_keeps_words_plain_and_marks_numbers_labels_and_paths():
    got = tokens("Error: BlinkyApi::led_off removed in 0.2.0, hash 5ff989a95162d4d7\n")
    assert got == [
        (Generic.Error, "Error"),
        (Name.Class, "BlinkyApi::led_off"),
        (Number, "0.2.0"),
        (Number.Hex, "5ff989a95162d4d7"),
    ]
    assert tokens("✔ staged in 1.9s\n") == [(Generic.Inserted, "✔"), (Number, "1.9")]
    assert tokens("warning the image is old\n") == [(Generic.Strong, "warning")]


def test_every_generated_transcript_lexes_to_its_text():
    generated = ROOT / "docs" / "generated"
    transcripts = [p for p in generated.rglob("*.txt") if p.read_text().startswith("$ ww ")]
    assert transcripts, "docs/generated/ has a ww transcript"
    for path in transcripts:
        got = tokens(path.read_text())
        assert (Name.Function, "ww") in got, path
