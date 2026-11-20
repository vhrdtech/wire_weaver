# common_error

Tests common errors (STD-16): a method with its own error type returns `Result<T, wire_weaver::Error<'_, E>>`, one without returns `Result<T, CommonError>`, and the client sees `CallError::Common` or `Error::common()`.

The same server also backs `inproc:errors-device` of `tests/hub_pytest_device`, for `wire_weaver_py`'s `test_errors.py`.
