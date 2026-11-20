# large_messages

Tests of messages over the device's scratch buffers and over the link's limit (CLT-55): a reply far longer
than `MAX_MESSAGE_LEN`, one over the device's own limit, one over the host's, and a stream of 10 000 rows that
fits neither as one message.

Exercises `large_messages_api::Archive` through the generated client, the local trait implementation, and the
device-side error path.
