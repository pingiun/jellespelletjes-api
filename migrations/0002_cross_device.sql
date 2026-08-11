-- Cross-device login: the requesting browser sends a random request id with
-- the magic-link request. If the link is later consumed in a browser that
-- does not hold that id (another device), the server issues a short 6-digit
-- transfer code instead of a session; the code is entered on the original
-- device to complete login there.
ALTER TABLE login_tokens ADD COLUMN request_hash BLOB;
ALTER TABLE login_tokens ADD COLUMN code_hash BLOB;
ALTER TABLE login_tokens ADD COLUMN code_attempts INTEGER NOT NULL DEFAULT 0;
