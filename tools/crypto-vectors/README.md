# crypto-vectors

`Gen.java` prints test vectors for `crates/protocol/src/crypto` by calling the real Java classes in the
oracle jar (cipher, Diffie-Hellman, token seeds, padding, and the unmodified `EncryptionEncoder` /
`EncryptionDecoder`). Rerun with `tools/crypto-vectors/run.sh [path/to/Havana-Server.jar]` (JDK 25; the
jar is `oracle/run/Havana-Server.jar` after `oracle/oracle.sh setup`); it rewrites
`crates/protocol/testdata/crypto-vectors.txt`. The codec ciphertexts contain random padding, so the file
changes on every run; the Rust tests only depend on its contents being consistent.
