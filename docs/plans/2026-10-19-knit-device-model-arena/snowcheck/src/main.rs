// snow 0.10 が IKpsk2 を実際に扱えるかの実証(knit の common と同じ feature 組)。
// 実行結果の各行が主張。失敗したら非ゼロ終了。
use snow::Builder;

fn main() {
    let params: snow::params::NoiseParams = "Noise_IKpsk2_25519_ChaChaPoly_BLAKE2s"
        .parse()
        .expect("IKpsk2 の名前解決に失敗");
    let legacy: snow::params::NoiseParams = "Noise_NNpsk0_25519_ChaChaPoly_BLAKE2s"
        .parse()
        .expect("現行 NNpsk0 の名前解決に失敗(これは絶対に通るはず)");
    println!("parse ok: {} / {}", params.name, legacy.name);

    let b = Builder::new(params.clone());
    let server = b.generate_keypair().expect("鍵生成");
    let client = b.generate_keypair().expect("鍵生成");
    let psk = [7u8; 32];

    let mut init = Builder::new(params.clone())
        .local_private_key(&client.private)
        .expect("initiator の静的鍵設定")
        .remote_public_key(&server.public)
        .expect("remote_public_key(IK に必須のはず)")
        .psk(2, &psk)
        .expect("psk(2, ..)")
        .prologue(b"knit-check-prologue")
        .expect("prologue")
        .build_initiator()
        .expect("IKpsk2 initiator 構築");

    let mut resp = Builder::new(params)
        .local_private_key(&server.private)
        .expect("responder の静的鍵設定")
        .psk(2, &psk)
        .expect("psk(2, ..)")
        .prologue(b"knit-check-prologue")
        .expect("prologue")
        .build_responder()
        .expect("IKpsk2 responder 構築");

    let mut buf1 = [0u8; 1024];
    let n1 = init
        .write_message(b"knit-init", &mut buf1)
        .expect("msg1 送信");
    println!("msg1 len = {n1}(新旧ディスパッチの判別用の実測値)");
    let mut out = [0u8; 1024];
    resp.read_message(&buf1[..n1], &mut out)
        .expect("responder が msg1 を復号");
    let remote_static = resp
        .get_remote_static()
        .expect("msg1 復号後に相手静的鍵を取得(台帳照合を msg2 前にやる前提)");
    assert_eq!(remote_static, client.public, "取得した静的鍵が client の公開鍵と一致");

    let mut buf2 = [0u8; 1024];
    let n2 = resp
        .write_message(b"knit-resp", &mut buf2)
        .expect("msg2 送信");
    println!("msg2 len = {n2}");
    init.read_message(&buf2[..n2], &mut out)
        .expect("initiator が msg2 を復号");

    let mut ti = init.into_transport_mode().expect("initiator transport");
    let mut tr = resp.into_transport_mode().expect("responder transport");
    let mut tbuf = [0u8; 1024];
    let m = ti.write_message(b"payload-after-handshake", &mut tbuf).expect("暗号化");
    let mut rout = [0u8; 1024];
    let r = tr.read_message(&tbuf[..m], &mut rout).expect("復号");
    assert_eq!(&rout[..r], b"payload-after-handshake");
    println!("transport 往復 ok");

    let other = Builder::new(legacy).generate_keypair().expect("第三者の鍵");
    let wrong_params: snow::params::NoiseParams = "Noise_IKpsk2_25519_ChaChaPoly_BLAKE2s"
        .parse()
        .expect("再パース");
    let mut wrong = Builder::new(wrong_params)
        .local_private_key(&other.private)
        .unwrap()
        .remote_public_key(&server.public)
        .unwrap()
        .psk(2, &psk)
        .unwrap()
        .prologue(b"knit-check-prologue")
        .unwrap()
        .build_initiator()
        .expect("第三者の initiator 構築(構築はできる)");
    let mut wbuf = [0u8; 1024];
    let wn = wrong.write_message(b"knit-init", &mut wbuf).expect("第三者 msg1 送信");
    let mut wresp = Builder::new(
        "Noise_IKpsk2_25519_ChaChaPoly_BLAKE2s"
            .parse()
            .expect("再パース"),
    )
    .local_private_key(&server.private)
    .unwrap()
    .psk(2, &psk)
    .unwrap()
    .prologue(b"knit-check-prologue")
    .unwrap()
    .build_responder()
    .expect("正規 responder 再構築");
    let mut wout = [0u8; 1024];
    wresp
        .read_message(&wbuf[..wn], &mut wout)
        .expect("第三者の msg1 も復号は成功する(IK の msg1 は静的鍵の正直な開示であり、鍵の否認は不可能)");
    let revealed = wresp
        .get_remote_static()
        .expect("開示された静的鍵の取得");
    assert_eq!(
        revealed, other.public,
        "msg1 が開示する静的鍵は真正(偽装不可)。未知=台帳照合で弾く対象であることの裏付け"
    );
    assert_ne!(revealed, client.public);
    println!("wrong-static reveals true key ok(拒否は暗号でなく台帳照合の役割)");
    println!("ALL OK");
}
