-- Client assertions already used at the token endpoint: SMART Backend Services requires
-- rejecting a repeated jti while the assertion is still valid.
CREATE TABLE IF NOT EXISTS oauth_assertion_jti (
    jti TEXT PRIMARY KEY,
    expires_at TIMESTAMPTZ NOT NULL
);

CREATE INDEX IF NOT EXISTS oauth_assertion_jti_expires ON oauth_assertion_jti (expires_at);
