//! Wire codec constants shipped in the user-supplied 日新工大 2.8.8 APK.
//! These are the shared client response decoder and server encryption public
//! key, NOT a user credential or a server signing key. They provide protocol
//! compatibility only; confidentiality/authenticity still require validated TLS.
//! Do not reuse this legacy 1024-bit codec to protect local passwords or backups.
pub(super) const RESPONSE_DECODER: &[u8] = br#"-----BEGIN PRIVATE KEY-----
MIICdwIBADANBgkqhkiG9w0BAQEFAASCAmEwggJdAgEAAoGBAJWKM5dGtdveeHF/f3sJ/aOqurdJokEtFxRMoEbR7UOlWkWeVTEr1BfcsCHOtGTqV14mEy1C7IIxH94fanqRqJWTs/0TKH/Qa6jBImW9Ts21ZRE+N0MQnMzta+uTYkK4YgNO00lLcToveZgOPP2tjOijOLU5/YEhm4SaP9XCLLIrAgMBAAECgYEAjPqKos6F+q/lCtNxgrSri5YUi2F+90UkIf4PiFS3A3QrA8E+fandPVXQMz8lcJJBJcBtidkzEZZwfb9OahlSPajYPsP71sH7tOUmn//KGRsUndI9MXr0QMEmviCLoTf4m7QCDhP+XPb7biEVJ1mDdb4Qs4IEWuR951CMQ5N9EOECQQDGvH4RRadb7zgB/sz42x0gkYukbzvNApLRKVLY+e2VMKLKP7TUGccCGR1gV8zcUHMtv2W1SI2W/YBrtX1QCr2xAkEAwKDNHRjzfMAQu3HztHyrQJAlfbF0z5JfT/N+aHf6oXSXtaXUzgDNpJB1lsx9dpzM1B6mkGmanc9xml2pnKdYmwJAW39a117nP5dyhNCn1AclcOIxlYI02R1PNQc+gnEG5kIfINiVy3UWv6uKb9nckq5jaPOOwxjlP1f1MSG80QYw8QJBAK/Z3IakwZvwZxYIKFhru5ccQO2ndCEO2i5N9ud+KHL+0oTE2CocN5/1NTQuiJcg/Cjltl992OYae/ZVbUMSzuMCQF7JusJUcYAZ6HLX2AGLau13vLgi+tEYmf/G4bdMiDpBWHYmLm1rbc6w9bBV4Q+LXw3oGpk39ksghlpnFKpLhJM=
-----END PRIVATE KEY-----"#;
pub(super) const REQUEST_PUBLIC_KEY: &[u8] = br#"-----BEGIN PUBLIC KEY-----
MIGfMA0GCSqGSIb3DQEBAQUAA4GNADCBiQKBgQDnA3aSX4wKyEgBXkZL0uASS0ejkULafAclMY2z3yIsJQNd2CG7Jj5erRUGb27C0JbnLwKUuCdVzbNNvhmSh8OO33aWXhdVnL8So6ffq3bHAAxUfq5eZJJB3mPM9GhxVu2e960YJXYiVCpDNFrUfpEsO0eSKg8zncc9UARDCHneKwIDAQAB
-----END PUBLIC KEY-----"#;
