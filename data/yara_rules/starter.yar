/*
 * AUGUR starter YARA rules.
 *
 * Super Sprint Group B P3. Forensic patterns commonly worth
 * surfacing in evidence content (translated or original).
 * Examiners are expected to load their own threat-intel rule
 * sets via `--yara-rules <path>`; this file ships a small
 * starter set so AUGUR is useful without any setup.
 */

rule bitcoin_wallet_address {
    meta:
        description = "Bitcoin wallet address pattern (legacy + bech32 prefixes)"
        forensic_value = "High"
        author = "AUGUR"
    strings:
        $btc_legacy = /\b[13][a-km-zA-HJ-NP-Z1-9]{25,34}\b/
        $btc_bech32 = /\bbc1[a-z0-9]{25,89}\b/ nocase
    condition:
        any of them
}

rule ethereum_wallet_address {
    meta:
        description = "Ethereum-style wallet address (0x + 40 hex)"
        forensic_value = "High"
        author = "AUGUR"
    strings:
        $eth = /\b0x[a-fA-F0-9]{40}\b/
    condition:
        $eth
}

rule url_pattern {
    meta:
        description = "HTTP/HTTPS URL detected in content"
        forensic_value = "Medium"
        author = "AUGUR"
    strings:
        $url = /https?:\/\/[^\s<>"']{8,512}/
    condition:
        $url
}

rule onion_address {
    meta:
        description = "Tor v2 / v3 .onion address"
        forensic_value = "High"
        author = "AUGUR"
    strings:
        $onion_v3 = /\b[a-z2-7]{56}\.onion\b/ nocase
        $onion_v2 = /\b[a-z2-7]{16}\.onion\b/ nocase
    condition:
        any of them
}

rule phone_number_intl {
    meta:
        description = "International phone number (E.164)"
        forensic_value = "Medium"
        author = "AUGUR"
    strings:
        $phone = /\+[1-9][0-9]{6,14}\b/
    condition:
        $phone
}

rule email_address {
    meta:
        description = "Email address pattern"
        forensic_value = "Medium"
        author = "AUGUR"
    strings:
        $email = /\b[A-Za-z0-9._%+-]{1,64}@[A-Za-z0-9.-]{1,253}\.[A-Za-z]{2,24}\b/
    condition:
        $email
}

rule ipv4_address {
    meta:
        description = "IPv4 address pattern"
        forensic_value = "Medium"
        author = "AUGUR"
    strings:
        $ipv4 = /\b(?:25[0-5]|2[0-4][0-9]|[01]?[0-9][0-9]?)(?:\.(?:25[0-5]|2[0-4][0-9]|[01]?[0-9][0-9]?)){3}\b/
    condition:
        $ipv4
}
