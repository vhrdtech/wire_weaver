"""The `bits` fence of the docs site (ww_docs_ext.bits)."""

import pytest
from pymdownx.superfences import SuperFencesException

from ww_docs_ext.bits import pack, parse, render

# the four IvSamples of docs/articles/series.md, bytes from ww_si's tests/series.rs `design_example`
IV = """
head  0100              n = 4
head  0010              IvSample field count = 2
run   0001 0            v.prefix run: Milli, end
run   0010 0            v.value tag run: U16, end
head  01 01             v.value column: delta, packed
value 0000110011100100  3300
width 00010             block width 2
delta 10 00 11          +1 0 -2 (zigzag 2 0 3)
run   0010 0            i.prefix run: Micro, end
run   0110 0            i.value tag run: I32, end
head  01 01             i.value column: delta, packed
value 00000000000000100101000111000000   152000
width 000011            block width 3
delta 100 101 100       +2 -3 +2 (zigzag 4 5 4)
pad   00                padding
bytes 42 11 14 33 90 51 91 8a 00 04 a3 80 1c b0
"""


def test_the_series_example_packs_to_its_bytes():
    fields, expected = parse(IV)
    assert pack("".join(f.bits for f in fields)) == expected
    assert [f.label for f in fields][:2] == ["n = 4", "IvSample field count = 2"]
    assert fields[7].groups == ["10", "00", "11"]


def test_render_draws_every_byte_and_field():
    out = render(IV)
    assert out.count('class="ww-bits__byte"') == 14
    assert out.count("<tr>") == 1 + 15
    assert 'ww-bit ww-bit--pad ww-bit--start" title="bit 110: padding"' in out
    assert '<span class="ww-bits__hex">b0</span>' in out


def test_label_starts_at_the_first_word_that_is_not_bits():
    fields, _ = parse("value 0101 1 3301 mV")
    assert fields[0].groups == ["0101", "1"]
    assert fields[0].label == "3301 mV"


def test_wrong_bytes_fail_the_build():
    with pytest.raises(SuperFencesException, match="the bytes line says"):
        render(IV.replace("1c b0", "1c b1"))


def test_bytes_need_whole_bytes():
    with pytest.raises(SuperFencesException, match="not whole bytes"):
        render("head 0100 n\nbytes 40")


def test_unknown_kind():
    with pytest.raises(SuperFencesException, match="unknown kind"):
        render("header 0100 n")


def test_labels_are_escaped_and_keep_code():
    out = render("head 0100 `n` < 8")
    assert "<td><code>n</code> &lt; 8</td>" in out
