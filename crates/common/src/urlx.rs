    //! Continue Here(ビジョン§11)で相手に開かせてよい URL の検査。
    //! 送信側・受信側の両方で同じ規則を適用する(片側だけの検査に頼らない)

    /// 相手に転送してよい URL か。http/https のみ・上限 2048 文字
    ///(file: 等のローカルスキームや過大なクエリを流さない)。
    /// 制御文字・空白の混入も拒否する(受信側の文字列がそのまま
    /// ShellExecuteW へ渡るため、改行入り等の偽装を塞ぐ)
    pub fn transferable(url: &str) -> bool {
        url.len() <= 2048
            && (url.starts_with("http://") || url.starts_with("https://"))
            && !url.chars().any(|c| c.is_control() || c.is_whitespace())
    }
