"""The scripts against the simulated thermometer: what each prints is what the book shows."""

from conftest import check_output

import channels
import observe
import properties
import streams
import units


def test_properties_prints_what_the_book_shows(sim_url):
    check_output("properties.py", properties.main, sim_url, "properties_py.txt")


def test_units_prints_what_the_book_shows(sim_url):
    check_output("units.py", units.main, sim_url, "units_py.txt")


def test_observe_prints_what_the_book_shows(sim_url):
    check_output("observe.py", observe.main, sim_url, "observe_py.txt")


def test_streams_prints_what_the_book_shows(sim_url):
    check_output("streams.py", streams.main, sim_url, "streams_py.txt")


def test_channels_prints_what_the_book_shows(sim_url):
    check_output("channels.py", channels.main, sim_url, "channels_py.txt")
