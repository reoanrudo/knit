#[test]
fn b64_roundtrip_all_cases() {
    let cases: Vec<Vec<u8>> = vec![
        vec![], vec![b'f'], vec![b'f', b'o'], b"foo".to_vec(), b"foob".to_vec(),
        b"fooba".to_vec(), b"foobar".to_vec(), vec![0xFFu8, 0x00, 0xFF],
        (0u8..=64).collect(),
    ];
    for src in cases {
        let got = sd_common::b64::encode(&src);
        let dec = sd_common::b64::decode(&got).expect("decode failed");
        assert_eq!(dec, src, "roundtrip mismatch for {:?}", src);
    }
    // 既知の標準ベクトル
    assert_eq!(sd_common::b64::encode(b"foobar"), "Zm9vYmFy");
    assert_eq!(sd_common::b64::encode(b"fo"), "Zm8=");
}
