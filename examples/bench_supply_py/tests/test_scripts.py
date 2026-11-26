"""The scripts against the simulated bench supply: what each prints is what the book shows."""

from conftest import check_output

import errors


def test_errors_prints_what_the_book_shows(sim_url):
    check_output("errors.py", errors.main, sim_url, "errors_py.txt")
