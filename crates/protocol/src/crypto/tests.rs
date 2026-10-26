//! Tests against vectors printed by the Java classes themselves
//! (`tools/crypto-vectors`, output in `testdata/crypto-vectors.txt`).

use std::collections::HashMap;
use std::sync::OnceLock;

use num_bigint::BigUint;

use super::*;

fn vectors() -> &'static HashMap<String, String> {
    static V: OnceLock<HashMap<String, String>> = OnceLock::new();
    V.get_or_init(|| {
        include_str!("../../testdata/crypto-vectors.txt")
            .lines()
            .filter_map(|l| l.split_once(" = "))
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    })
}

fn v(name: &str) -> String {
    vectors()
        .get(name)
        .unwrap_or_else(|| panic!("no vector {name}"))
        .clone()
}

fn unhex(s: &str) -> Vec<u8> {
    (0..s.len() / 2)
        .map(|i| u8::from_str_radix(&s[2 * i..2 * i + 2], 16).unwrap())
        .collect()
}

fn big(name: &str) -> BigUint {
    v(name).parse().unwrap()
}

/// A scripted random source: cycles through `values`, each taken modulo the bound.
fn scripted(values: &'static [u32]) -> impl Random {
    let mut i = 0;
    move |bound: u32| {
        let r = values[i % values.len()] % bound;
        i += 1;
        r
    }
}

#[test]
fn group_parameters_match_java() {
    let (g, p) = parameters();
    assert_eq!(g, big("dh.g"));
    assert_eq!(p, big("dh.p"));
}

#[test]
fn security_code_decode_matches_java() {
    for x in [0, 1, 5678, 7639, 12345, 32767] {
        assert_eq!(
            security_decode(x),
            v(&format!("securitycode.decode.{x}"))
                .parse::<i32>()
                .unwrap(),
            "decode({x})"
        );
    }
}

#[test]
fn diffie_hellman_matches_java() {
    for i in 0..4 {
        for j in 0..4 {
            if i == j {
                continue;
            }
            let n = format!("dh.{i}.{j}");
            let server =
                DiffieHellman::new(v(&format!("{n}.server_private")).parse().unwrap()).unwrap();
            let client =
                DiffieHellman::new(v(&format!("{n}.client_private")).parse().unwrap()).unwrap();
            assert_eq!(server.public_key(), v(&format!("{n}.server_public")));
            assert_eq!(client.public_key(), v(&format!("{n}.client_public")));
            let expected = unhex(&v(&format!("{n}.key")));
            // GENERATEKEY on the server, and the client's mirror of it.
            assert_eq!(
                server.shared_key(client.public_key().as_bytes()).unwrap(),
                expected
            );
            assert_eq!(
                client.shared_key(server.public_key().as_bytes()).unwrap(),
                expected
            );
            assert_eq!(
                key_bytes(&big(&format!("{n}.shared"))),
                expected,
                "key bytes of the shared secret"
            );
        }
    }
}

#[test]
fn diffie_hellman_rejects_bad_input() {
    assert!(DiffieHellman::new(0).is_none());
    let dh = DiffieHellman::new(7).unwrap();
    assert_eq!(dh.shared_key(b""), Err(CryptoError::BadPublicKey));
    assert_eq!(dh.shared_key(b"12x"), Err(CryptoError::BadPublicKey));
    // 0 as the peer key gives a zero secret, which has no key bytes.
    assert_eq!(dh.shared_key(b"0"), Err(CryptoError::EmptyKey));
}

#[test]
fn key_bytes_drop_the_sign_byte() {
    for s in [
        "1", "127", "128", "255", "256", "32768", "65535", "16777216",
    ] {
        let n: BigUint = s.parse().unwrap();
        assert_eq!(key_bytes(&n), unhex(&v(&format!("keybytes.{s}"))), "{s}");
    }
}

#[test]
fn cipher_matches_java() {
    for k in 0..4 {
        let key = unhex(&v(&format!("cipher.{k}.key")));
        let mut enc = Cipher::new(&key).unwrap();
        let mut dec = Cipher::new(&key).unwrap();
        for m in 0..8 {
            let plain = unhex(&v(&format!("cipher.{k}.{m}.plain")));
            let encoded = v(&format!("cipher.{k}.{m}.encoded"));
            // One instance for all messages: the keystream carries over.
            assert_eq!(enc.encode(&plain), encoded.as_bytes(), "encode {k}.{m}");
            assert_eq!(
                dec.decode(encoded.as_bytes()).unwrap(),
                plain,
                "decode {k}.{m}"
            );
        }
    }
}

#[test]
fn cipher_decodes_every_length_class() {
    let key = unhex(&v("cipher.0.key"));
    for len in [2, 3, 4, 6, 7, 8, 10, 11, 12, 64] {
        let input = v(&format!("decode.{len}.input"));
        let out = Cipher::new(&key).unwrap().decode(input.as_bytes()).unwrap();
        assert_eq!(out, unhex(&v(&format!("decode.{len}.output"))), "len {len}");
    }
}

#[test]
fn cipher_rejects_bad_text() {
    let mut c = Cipher::new(&[1, 2, 3]).unwrap();
    assert_eq!(c.decode(b"AB!D"), Err(CryptoError::InvalidChar(b'!')));
    assert_eq!(c.decode(b"ABCDE"), Err(CryptoError::TruncatedText));
    assert!(Cipher::new(&[]).is_err());
}

#[test]
fn token_seeds_and_counter_match_java() {
    for t in 0..5 {
        let token = v(&format!("token.{t}"));
        let (tx, rx) = seeds_from_token(&token).unwrap();
        assert_eq!(
            tx,
            v(&format!("token.{t}.tx")).parse::<u32>().unwrap(),
            "tx {token}"
        );
        assert_eq!(
            rx,
            v(&format!("token.{t}.rx")).parse::<u32>().unwrap(),
            "rx {token}"
        );
    }
    let mut s = 0x1234;
    let seq: Vec<u32> = (0..12)
        .map(|_| {
            s = iterate_random(s);
            s
        })
        .collect();
    let expected: Vec<u32> = v("iterate.from_4660")
        .split(' ')
        .map(|x| x.parse().unwrap())
        .collect();
    assert_eq!(seq, expected);
    assert_eq!(
        iterate_random(65535),
        v("iterate.65535").parse::<u32>().unwrap()
    );
    assert!(seeds_from_token("12").is_err());
    assert!(seeds_from_token("12zz").is_err());
}

#[test]
fn padding_matches_java() {
    for (len, pad) in [(0, 0), (3, 0), (3, 2), (3, 3), (3, 4), (1, 5), (10, 4)] {
        let body = &b"abcdefghij"[..len];
        assert_eq!(
            remove_padding(body, pad),
            unhex(&v(&format!("removepad.{len}.{pad}")))
        );
    }
    for n in 0..5 {
        let padded = add_padding(b"xyz", n, &mut scripted(&[0, 254, 100]));
        assert_eq!(
            padded.len(),
            v(&format!("addpad.{n}.length")).parse::<usize>().unwrap()
        );
        assert_eq!(&padded[n..], unhex(&v(&format!("addpad.{n}.suffix"))));
        assert!(padded[..n].iter().all(|&b| b != 0));
    }
}

/// Java's `EncryptionEncoder` output, decrypted by the client end, gives back the frames.
#[test]
fn client_decrypts_frames_of_the_java_encoder() {
    let key = unhex(&v("codec.key"));
    for t in 0..2 {
        let token = v(&format!("codec.{t}.token"));
        let count: usize = v(&format!("codec.{t}.server.count")).parse().unwrap();
        let mut whole = Crypto::client(&key, &token).unwrap();
        let mut bytewise = Crypto::client(&key, &token).unwrap();
        for i in 0..count {
            let plain = unhex(&v(&format!("codec.{t}.server.{i}.plain")));
            let enc = unhex(&v(&format!("codec.{t}.server.{i}.encrypted")));
            assert_eq!(whole.decrypt(&enc).unwrap(), plain, "token {t} frame {i}");
            // Split at every byte, the case where Java's decoder re-reads the header.
            let mut got = Vec::new();
            for b in &enc {
                got.extend(bytewise.decrypt(&[*b]).unwrap());
            }
            assert_eq!(got, plain, "bytewise, token {t} frame {i}");
        }
    }
}

/// Frames the Java encoder made for a client, decrypted by the server end like Java's decoder.
#[test]
fn server_decrypts_frames_like_the_java_decoder() {
    let key = unhex(&v("codec.key"));
    for t in 0..2 {
        let token = v(&format!("codec.{t}.token"));
        let count: usize = v(&format!("codec.{t}.client.count")).parse().unwrap();
        let mut crypto = Crypto::server(&key, &token).unwrap();
        let mut all_encrypted = Vec::new();
        let mut all_decoded = Vec::new();
        for i in 0..count {
            let enc = unhex(&v(&format!("codec.{t}.client.{i}.encrypted")));
            let decoded = unhex(&v(&format!("codec.{t}.client.{i}.decoded")));
            let payload = unhex(&v(&format!("codec.{t}.client.{i}.payload")));
            // What the Java decoder emitted is B64(3) length + payload.
            let mut expected = b64_encode(payload.len() as u32, 3);
            expected.extend(&payload);
            assert_eq!(decoded, expected, "vector sanity");
            assert_eq!(
                crypto.decrypt(&enc).unwrap(),
                decoded,
                "token {t} frame {i}"
            );
            all_encrypted.extend(enc);
            all_decoded.extend(decoded);
        }
        // All frames in one read must give the same bytes.
        let mut crypto = Crypto::server(&key, &token).unwrap();
        assert_eq!(crypto.decrypt(&all_encrypted).unwrap(), all_decoded);
    }
}

/// The two ends of this module agree with each other, with real padding choices and every
/// counter phase, and the server end's output has the shape the Java decoder expects.
#[test]
fn ends_round_trip() {
    let key = unhex(&v("codec.key"));
    let token = "391245501337402612345678";
    let mut server = Crypto::server(&key, token).unwrap();
    let mut client = Crypto::client(&key, token).unwrap();
    let mut rng = scripted(&[7, 200, 33, 126, 254, 1, 99]);
    for i in 0..40u16 {
        let msg = crate::frame::Message::new(i, vec![b'x'; usize::from(i) * 7]);
        let wire = server
            .encrypt(&crate::frame::encode_server(&msg), &mut rng)
            .unwrap();
        // 6 header characters + body characters, all from the cipher alphabet.
        assert!(
            wire.iter()
                .all(|b| b.is_ascii_alphanumeric() || *b == b'+' || *b == b'/')
        );
        let mut d = crate::frame::Decoder::default();
        d.push(&client.decrypt(&wire).unwrap());
        assert_eq!(d.next_server(), Some(msg.clone()), "server -> client {i}");

        let wire = client
            .encrypt(&crate::frame::encode_client(&msg), &mut rng)
            .unwrap();
        let mut d = crate::frame::Decoder::default();
        d.push(&server.decrypt(&wire).unwrap());
        assert_eq!(d.next_client(), Some(msg), "client -> server {i}");
    }
}

#[test]
fn encoder_output_is_java_cipher_of_padded_payload() {
    // With the counter at a known phase the encoder is exactly Cipher::encode(pad + payload)
    // behind a header; the first frame of token 0 has rx = 0x1234 -> iterate -> pad length 0.
    let key = unhex(&v("codec.key"));
    let mut enc = FrameEncoder::new(&key, 0x1234).unwrap();
    let wire = enc.encode(b"hello", &mut scripted(&[41])).unwrap();
    let mut body = Cipher::new(&key).unwrap();
    let mut header = Cipher::new(&key).unwrap();
    let pad = (iterate_random(0x1234) % 5) as usize;
    let padded = add_padding(b"hello", pad, &mut scripted(&[41]));
    let body_text = body.encode(&padded);
    let len = body_text.len();
    let h = header.encode(&[42, 64, 64 | (len / 64) as u8, 64 | (len & 63) as u8]);
    let mut expected = h;
    expected.extend(body_text);
    assert_eq!(wire, expected);
}

#[test]
fn oversized_frames_are_rejected() {
    let key = [1u8, 2, 3];
    let mut enc = FrameEncoder::new(&key, 0).unwrap();
    let r = enc.encode(&vec![0u8; 200_000], &mut scripted(&[1]));
    assert!(matches!(r, Err(CryptoError::FrameTooLong(_))));
}

#[test]
fn token_from_crypto_parameters_body() {
    assert_eq!(
        token_from_crypto_parameters(b"123456\x02I").as_deref(),
        Some("123456")
    );
    assert_eq!(token_from_crypto_parameters(b"no terminator"), None);
}
