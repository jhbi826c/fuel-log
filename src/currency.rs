use crate::i18n::Lang;

pub struct Currency {
    pub code: &'static str,
    pub symbol: &'static str,
    en: &'static str,
    zh: &'static str,
    de: &'static str,
}

impl Currency {
    pub fn name(&self, lang: Lang) -> &'static str {
        match lang {
            Lang::En => self.en,
            Lang::Zh => self.zh,
            Lang::De => self.de,
        }
    }
}

const fn c(code: &'static str, symbol: &'static str, en: &'static str, zh: &'static str, de: &'static str) -> Currency {
    Currency { code, symbol, en, zh, de }
}

pub const CURRENCIES: &[Currency] = &[
    c("EUR", "€", "Euro", "欧元", "Euro"),
    c("USD", "$", "US Dollar", "美元", "US-Dollar"),
    c("GBP", "£", "British Pound", "英镑", "Britisches Pfund"),
    c("CHF", "CHF", "Swiss Franc", "瑞士法郎", "Schweizer Franken"),
    c("CZK", "Kč", "Czech Koruna", "捷克克朗", "Tschechische Krone"),
    c("PLN", "zł", "Polish Złoty", "波兰兹罗提", "Polnischer Złoty"),
    c("HUF", "Ft", "Hungarian Forint", "匈牙利福林", "Ungarischer Forint"),
    c("SEK", "kr", "Swedish Krona", "瑞典克朗", "Schwedische Krone"),
    c("NOK", "kr", "Norwegian Krone", "挪威克朗", "Norwegische Krone"),
    c("DKK", "kr", "Danish Krone", "丹麦克朗", "Dänische Krone"),
    c("ISK", "kr", "Icelandic Króna", "冰岛克朗", "Isländische Krone"),
    c("RON", "lei", "Romanian Leu", "罗马尼亚列伊", "Rumänischer Leu"),
    c("TRY", "₺", "Turkish Lira", "土耳其里拉", "Türkische Lira"),
    c("UAH", "₴", "Ukrainian Hryvnia", "乌克兰格里夫纳", "Ukrainische Hrywnja"),
    c("RUB", "₽", "Russian Ruble", "俄罗斯卢布", "Russischer Rubel"),
    c("CNY", "¥", "Chinese Yuan", "人民币", "Chinesischer Yuan"),
    c("JPY", "¥", "Japanese Yen", "日元", "Japanischer Yen"),
    c("HKD", "HK$", "Hong Kong Dollar", "港币", "Hongkong-Dollar"),
    c("TWD", "NT$", "New Taiwan Dollar", "新台币", "Neuer Taiwan-Dollar"),
    c("KRW", "₩", "South Korean Won", "韩元", "Südkoreanischer Won"),
    c("SGD", "S$", "Singapore Dollar", "新加坡元", "Singapur-Dollar"),
    c("INR", "₹", "Indian Rupee", "印度卢比", "Indische Rupie"),
    c("THB", "฿", "Thai Baht", "泰铢", "Thailändischer Baht"),
    c("MYR", "RM", "Malaysian Ringgit", "马来西亚林吉特", "Malaysischer Ringgit"),
    c("IDR", "Rp", "Indonesian Rupiah", "印尼盾", "Indonesische Rupiah"),
    c("PHP", "₱", "Philippine Peso", "菲律宾比索", "Philippinischer Peso"),
    c("VND", "₫", "Vietnamese Dong", "越南盾", "Vietnamesischer Dong"),
    c("AUD", "A$", "Australian Dollar", "澳元", "Australischer Dollar"),
    c("NZD", "NZ$", "New Zealand Dollar", "新西兰元", "Neuseeland-Dollar"),
    c("CAD", "CA$", "Canadian Dollar", "加元", "Kanadischer Dollar"),
    c("MXN", "MX$", "Mexican Peso", "墨西哥比索", "Mexikanischer Peso"),
    c("BRL", "R$", "Brazilian Real", "巴西雷亚尔", "Brasilianischer Real"),
    c("ZAR", "R", "South African Rand", "南非兰特", "Südafrikanischer Rand"),
    c("AED", "AED", "UAE Dirham", "阿联酋迪拉姆", "VAE-Dirham"),
    c("ILS", "₪", "Israeli Shekel", "以色列新谢克尔", "Israelischer Schekel"),
];

/// Currencies matching `query` by code, symbol or name in any language.
/// Exact code/symbol hits come first, then prefix hits, then the rest.
pub fn search(query: &str) -> Vec<&'static Currency> {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return CURRENCIES.iter().collect();
    }
    let rank = |c: &Currency| {
        let (code, symbol) = (c.code.to_lowercase(), c.symbol.to_lowercase());
        let names = [c.en, c.zh, c.de].map(str::to_lowercase);
        if code == q || symbol == q {
            Some(0)
        } else if code.starts_with(&q) || symbol.starts_with(&q) || names.iter().any(|n| n.starts_with(&q)) {
            Some(1)
        } else if names.iter().any(|n| n.contains(&q)) {
            Some(2)
        } else {
            None
        }
    };
    let mut hits: Vec<_> = CURRENCIES.iter().filter_map(|c| rank(c).map(|r| (r, c))).collect();
    hits.sort_by_key(|(r, _)| *r);
    hits.into_iter().map(|(_, c)| c).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn codes(q: &str) -> Vec<&'static str> {
        search(q).into_iter().map(|c| c.code).collect()
    }

    #[test]
    fn empty_query_lists_all() {
        assert_eq!(search("  ").len(), CURRENCIES.len());
    }

    #[test]
    fn matches_code_symbol_and_names() {
        assert_eq!(codes("usd"), ["USD"]);
        assert_eq!(codes("Kč"), ["CZK"]);
        assert_eq!(codes("人民币"), ["CNY"]);
        assert_eq!(codes("franken"), ["CHF"]);
        assert!(codes("dollar").contains(&"CAD"));
        assert!(codes("xyz").is_empty());
    }

    #[test]
    fn exact_hits_rank_first() {
        assert_eq!(codes("r")[0], "ZAR");
        assert_eq!(codes("kr")[..4], ["SEK", "NOK", "DKK", "ISK"]);
    }
}
