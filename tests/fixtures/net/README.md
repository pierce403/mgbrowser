# Local TLS fixtures

The DER certificate chain and PKCS#8 private key are deliberately public test
fixtures. They are trusted only by a local test client. The browser's real root
store never trusts this test CA. The leaf covers `localhost`, not `127.0.0.1`,
so the same chain also verifies hostname rejection.

Generated locally using OpenSSL's command-line certificate tooling, with P-256
keys, SHA-256 signatures, and 100-year validity starting September 7, 2026.
OpenSSL is not a browser dependency and is not called by the browser or tests.
Both sides of the actual local handshake tests use RustCrypto through rustls.
`localhost.cnf` preserves the leaf extensions used during generation.
