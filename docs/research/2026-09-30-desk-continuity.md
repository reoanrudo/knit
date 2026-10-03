# 「机全体をひとつに」調査の取り込みと、モニター入力切替(DDC)を採用しない理由

作成: 2026-09-30。利用者が持ち込んだ市場調査の結論を、既存の製品判断([essence-review](2026-09-28-essence-review.md)、
[製品定義](../product-definition.md)、[ロードマップ](../plans/2026-09-30-global-commercialization-roadmap.md))
に照らして仕分けた記録。**調査の数値・引用(Reddit の票数、各競合の対応状況など)は持ち込まれた情報で、この文書では再確認していない。**
DDC の実現性だけは公式資料と一次情報で確かめ、その結果もあって採用しないと決めた。

## 持ち込まれた調査の要点

- 需要の中心は「複数 PC の共有」より「仕事 PC・私物 PC・複数モニター・周辺機器を、ひとつの机として使う」こと。
  現状は KVM・USB スイッチ・ドック・モニター入力切替・Stream Deck・ソフト KVM を組み合わせている
- 最優先の不満は、スリープ復帰・Wi-Fi 変更・OS 更新の後に手作業で再接続が要ること(自動復旧)
- 未充足の需要: ボタン 1 つで机の状態を切り替える「Desk Scene」、高リフレッシュレートを落とさないモニター入力の切替、
  会社 PC には入力だけ渡す信頼ゾーン、Windows/Android からタブレットへ続けてカーソルを動かすこと

## 判断(2026-09-30、利用者)

| 案 | 判断 | 理由 |
|---|---|---|
| 自動復旧・入力の品質 | **継続(P0)** | ロードマップ Q2/Q6 と一致。信頼性が最上位の需要でもある |
| 信頼ゾーン(端末ごとの共有範囲) | **優先度を上げる** | Q5 の端末別権限の具体化。会社 PC を取り込む条件になる |
| Desk Scene(机の状態の切替)・モニター入力の DDC 切替 | **採用しない** | 需要は強いが、モニター・接続経路ごとの機種差が大きく、確実に動くと保証できない。誤った入力への切替で画面が消える事故もあり、「毎日確実に何も考えずに」という本質(信頼を削らない)に反する。Knit の境界はキーボード・ポインター・持ち物の受け渡しまでとし、映像の経路は扱わない |
| Desk Search(端末横断の検索) | **採用しない** | 2026-09-28 に「いらない機能」として撤去済み。復活させない |
| タブレットを PC の追加画面・周辺機器にする | **採用しない** | 「各機器は自分の OS のまま使う」原則と趣旨が違う |
| ウェブカメラ・マイク・Stream Deck 等の USB 機器の付け替え | **対象外** | USB 機器は物理的に接続された PC にしか見えず、ソフトだけでは移せない。USB スイッチの領域 |

## 参考: DDC でモニター入力を切り替える実現性(採用しない判断の根拠)

結論: 技術的には可能だが、「どのモニターでも効く」とは言えない。対応機種と接続経路の確認が製品の中身になってしまうため、採用しない。

| 論点 | 確認した事実 | 出典 |
|---|---|---|
| 標準の仕組み | MCCS の入力切替は VCP コード 0x60。Windows は `SetVCPFeature`(Dxva2)で送れる。ただし多くのモニターは MCCS を部分的にしか実装せず、Microsoft も未検証の機種での使用を勧めていない。入力の値(例: DisplayPort=15、USB-C=26)は機種ごとに違い、能力文字列で確認する | [Microsoft: SetVCPFeature](https://learn.microsoft.com/en-us/windows/win32/api/lowlevelmonitorconfigurationapi/nf-lowlevelmonitorconfigurationapi-setvcpfeature)、[DPSwitcher](https://github.com/notKleja/DPSwitcher) |
| 書き込みが成功しても切り替わらない機種 | LG UltraGear の一部は 0x60 を受け付けて成功を返すが入力は変わらず、独自の VCP(0xF4)と別アドレス(0x50)を使うと報告されている。Windows 標準 API では送れない | [deskmux](https://github.com/klaidliadon/deskmux) |
| Apple Silicon Mac | 内蔵の HDMI ポートは DDC/CI が通らない機種が多い(M1 系 Mac mini・Studio など。M3 Pro でも報告あり)。USB-C/Thunderbolt(DisplayPort Alt Mode)経由なら通る。m1ddc は内蔵 HDMI を扱えない | [m1ddc](https://github.com/waydabber/m1ddc)、[MonitorControl の議論](https://github.com/MonitorControl/MonitorControl/discussions/750) |
| 入力が切れた側からは送れない | 多くのモニターは、現在表示中の入力につながった PC からの DDC/CI しか受け付けない。したがって「相手へ移る時に、自分が今のうちに『相手の入力へ切替』を送る」形が基本になる(相手側から「自分へ切替」を送る形は機種依存) | [bad_kvm_switch](https://github.com/christocs/bad_kvm_switch)、[tqdev](https://www.tqdev.com/2025-usb-soft-kvm-monitor-switching-ddc-ci/) |
| 経路の影響 | ドック・アダプタ・KVM を挟むと DDC が通らないことがある。誤った入力へ切り替えて画面が真っ暗になり、モニター本体の操作が必要になる事故がある | [Display Dimmer](https://displaydimmer.com/ddc-ci-not-working-hdmi-switch-kvm)、[ddcutil issue](https://github.com/rockowitz/ddcutil/issues/566) |
| 競合の状況 | ShareMouse はバージョン 6 以降、共有ディスプレイの入力切替に対応するが、自社文書で「試験的な機能で製品仕様外」と明記し、機種差が大きいと注意している | [ShareMouse: 共有ディスプレイ](https://www.sharemouse.com/doc/manage/shared-display/) |

### 将来この判断を見直す条件

利用者が「モニターの入力切替まで Knit でやってほしい」と繰り返し求め、かつ確認済みの機種で確実に動くと
実機で示せる場合に限り、既定オフの実験機能として再検討する。それまでは作らない。

### 未検証

- 実機での動作(実装も試験もしていない)
- 持ち込まれた調査の数値と競合の対応状況(この文書では再確認していない)
