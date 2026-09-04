#[test]
fn probe() {
    let words = [
        "মেঘ",      // megh
        "বাতাস",    // batash
        "কথা",      // kotha
        "ভাল",      // bhalo
        "মন",       // mon
        "প্রেম",    // prem
        "ধর্ম",     // dhormo
        "ক্ষণ",     // khon
        "আত্মা",    // atta
        "বাংলা",    // bangla
        "দুঃখ",    // dukkho
        "রাত্রি",   // ratri
        "শ্রেষ্ঠ",  // shrestho
        "স্বজন",    // shojon
        "কর্তব্য",  // kortobbo
        "আমি",      // ami
        "তুমি",     // tumi
        "সে",       // she
        "আকাশ",    // akash
        "পৃথিবী",  // prithibi
        "জীবন",    // jibon
        "পানি",    // pani
        "বই",      // boi
        "গান",     // gan
        "দেশ",     // desh
        "রাস্তা",  // rasta
        "শহর",     // shohor
        "ব্রাহ্মণ", // brammon
        "দুর্গ",   // durg
        "ডাক্তার", // daktar
        "ডাকলাম", // daklam
        "গোলাপ",  // golap
        "পড়াশোনা", // porashona
        "বললাম",  // bollam
        "যাচ্ছি", // jacchi
        "আসছি",  // aschi
        "করেছি",  // korechi
        "দেখছি",  // dekhchi
        "মানুষ",  // manush
        "কেমন",   // kemon
        "আচ্ছা",  // accha
        "মিষ্টি", // mishti
        "কাছাকাছি", // kachakachi
        "চলছে",   // cholche
        "আনন্দ",  // anondo
        "পরিবার", // poribar
        "সময়",   // shomoy
    ];
    for w in words {
        println!("{w} -> {}", bntomew::transliterate(w).romanized);
    }
}
