    /// Mac keycode(HIToolbox)→ Windows 仮想キーコード(VK)
    pub fn mac_kc_to_win_vk(kc: u16) -> Option<u16> {
        let vk = match kc {
            // アルファベット(Mac keycode はレイアウト依存しない物理キー)
            0 => 0x41,  // A
            11 => 0x42, // B
            8 => 0x43,  // C
            2 => 0x44,  // D
            14 => 0x45, // E
            3 => 0x46,  // F
            5 => 0x47,  // G
            4 => 0x48,  // H
            34 => 0x49, // I
            38 => 0x4A, // J
            40 => 0x4B, // K
            37 => 0x4C, // L
            46 => 0x4D, // M
            45 => 0x4E, // N
            31 => 0x4F, // O
            35 => 0x50, // P
            12 => 0x51, // Q
            15 => 0x52, // R
            1 => 0x53,  // S
            17 => 0x54, // T
            32 => 0x55, // U
            9 => 0x56,  // V
            13 => 0x57, // W
            7 => 0x58,  // X
            16 => 0x59, // Y
            6 => 0x5A,  // Z
            // 数字row
            18 => 0x31, // 1
            19 => 0x32,
            20 => 0x33,
            21 => 0x34,
            23 => 0x35,
            22 => 0x36,
            26 => 0x37,
            28 => 0x38,
            25 => 0x39,
            29 => 0x30, // 0
            // 記号
            33 => 0xDB, // [
            30 => 0xDD, // ]
            39 => 0xBA, // ;
            41 => 0xDE, // '
            42 => 0xDC, // \
            43 => 0xBC, // ,
            47 => 0xBE, // .
            44 => 0xBF, // /
            50 => 0xC0, // `
            // ¥(Mac JIS 単体キー)→ Win の 0xDC(\ キー)。Windows の JIS 配列に
            // ¥ 単体キーは無いため半角 \ として届く(全角￥は Windows 側の IME の
            // 変換候補で。設定化は将来課題)
            93 => 0xDC,
            27 => 0xBD, // -(US)/ー(JIS 長音)→ Win -[OEM_MINUS]
            94 => 0xBD, // _(Mac JIS)→ Win -
            // 制御・編集
            36 => 0x0D,  // Return
            48 => 0x09,  // Tab
            49 => 0x20,  // Space
            51 => 0x08,  // Delete(Backspace)
            53 => 0x1B,  // Escape
            117 => 0x2E, // Forward Delete
            115 => 0x24, // Home
            119 => 0x23, // End
            116 => 0x21, // PageUp
            121 => 0x22, // PageDown
            123 => 0x25, // Left
            124 => 0x27, // Right
            125 => 0x28, // Down
            126 => 0x26, // Up
            // Fキー
            122 => 0x70, // F1
            120 => 0x71, // F2
            99 => 0x72,  // F3
            118 => 0x73, // F4
            96 => 0x74,  // F5
            97 => 0x75,  // F6
            98 => 0x76,  // F7
            100 => 0x77, // F8
            101 => 0x78, // F9
            109 => 0x79, // F10
            103 => 0x7A, // F11
            111 => 0x7B, // F12
            // テンキー
            82 => 0x60, // Num0
            83 => 0x61,
            84 => 0x62,
            85 => 0x63,
            86 => 0x64,
            87 => 0x65,
            88 => 0x66,
            89 => 0x67,
            91 => 0x68,
            92 => 0x69,  // Num9
            65 => 0x6E,  // Num .
            67 => 0x6A,  // Num *
            69 => 0x6B,  // Num +
            78 => 0x6D,  // Num -
            75 => 0x6F,  // Num /
            71 => 0x0C,  // Clear
            76 => 0x0D,  // テンキー Enter(Win 側で拡張キーフラグを付けて区別する)
            57 => 0x14,  // Caps Lock(Mac 側は押下ごとに down+up の組で送る)
            114 => 0x2D, // Help(Mac の Ins 位置)→ Insert
            // かな(104)/英数(102)はここを通らない: win 側の受信ループが先に
            // 傍受して ime_set_open(IME 開閉)へ変換する
            // F13〜F20(F13 は既定ホットキーのため通常は Mac 側で握られる)
            105 => 0x7C,
            107 => 0x7D,
            113 => 0x7E,
            106 => 0x7F,
            64 => 0x80,
            79 => 0x81,
            80 => 0x82,
            90 => 0x83,
            _ => return None,
        };
        Some(vk)
    }

    /// Caps Lock の越境同期: Windows 側の現在のトグル状態と Mac 側の
    /// alphaShift を比べ、合わせるためのトグル注入(1 回)が要るか。
    /// Windows の Caps Lock は「押すたびに反転」のため、状態が既に一致して
    /// いる時に注入すると逆にズレる。Vk 57→VK_CAPITAL(0x14) と同じく
    /// 両 OS の対応を 1 箇所で保証するためにここへ置く
    pub fn caps_toggle_needed(win_caps_on: bool, mac_caps_on: bool) -> bool {
        win_caps_on != mac_caps_on
    }

    /// 切替キー(Mac keycode)の表示名。設定画面の「切替キー（…）のみ」の項目名など
    /// 両 OS の設定 UI で同じ名前を出すための変換。候補に無いキーは「コードN」
    /// (Mac 側の「現在のキー（コードN）」表示と同じ規則)
    pub fn mac_key_label(kc: i64) -> String {
        match kc {
            97 => "F6".into(),
            100 => "F8".into(),
            105 => "F13".into(),
            54 => "右⌘".into(),
            other => format!("コード{other}"),
        }
    }

    #[cfg(test)]
    mod caps_sync_tests {
        use super::caps_toggle_needed;

        #[test]
        fn injects_only_when_the_states_differ() {
            // 状態が異なる時だけ 1 回トグル注入が必要
            assert!(caps_toggle_needed(true, false), "Win ON / Mac OFF は注入要");
            assert!(caps_toggle_needed(false, true), "Win OFF / Mac ON は注入要");
        }

        #[test]
        fn skips_injection_when_already_aligned() {
            // 既に一致している時に注入すると逆にズレるため注入しない
            assert!(!caps_toggle_needed(true, true), "両方 ON はそのまま");
            assert!(!caps_toggle_needed(false, false), "両方 OFF はそのまま");
        }
    }

    #[cfg(test)]
    mod key_label_tests {
        use super::mac_key_label;

        /// 設定 UI の候補(F6/F8/F13/右⌘)はキー名、それ以外は「コードN」。
        /// 両 OS で同じ文言になることをここで固定する
        #[test]
        fn candidates_have_names_and_others_use_code_form() {
            assert_eq!(mac_key_label(97), "F6");
            assert_eq!(mac_key_label(100), "F8");
            assert_eq!(mac_key_label(105), "F13");
            assert_eq!(mac_key_label(54), "右⌘");
            assert_eq!(mac_key_label(63), "コード63");
            assert_eq!(mac_key_label(0), "コード0");
        }
    }
