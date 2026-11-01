# 缂栬瘧鎸囧崡

## 1. 鐜

| 缁勪欢 | 瑕佹眰 |
| --- | --- |
| 绯荤粺 | Windows 10 2004+ / Windows 11锛寈64 |
| Rust | stable锛宔dition 2024锛屽缓璁?1.85+锛堟湰浠撳簱鐢?1.99.1 瀹炴祴锛?|
| 閾炬帴鍣?| Visual Studio 2022/2026 鐨勨€滀娇鐢?C++ 鐨勬闈㈠紑鍙戔€濆伐浣滆礋杞斤紙MSVC + Windows SDK锛?|
| 缃戠粶 | 棣栨 `cargo fetch` 闇€瑕佽闂?crates.io 鎴栭暅鍍?|

`cargo` 蹇呴』鑳芥壘鍒?`link.exe`銆傚鏋?`cargo build` 鎶?`link.exe not found`锛岃浠?鈥渪64 Native Tools Command Prompt for VS鈥?鍚姩锛屾垨瀹夎 VS Build Tools銆?
### 鍥藉唴 crates 闀滃儚锛堝彲閫夛級

鍦?`%USERPROFILE%\.cargo\config.toml` 涓姞鍏ワ細

```toml
[source.crates-io]
replace-with = 'rsproxy-sparse'

[source.rsproxy-sparse]
registry = "sparse+https://rsproxy.cn/index/"
```

## 2. 缂栬瘧涓庢祴璇?
鍦?`IslandVoice/` 鐩綍鎵ц锛?
```powershell
cargo fetch
cargo test
cargo build --release
```

浜х墿锛?
```text
target/release/island_voice.dll
```

鍙€夛細ABI 鐢熷懡鍛ㄦ湡鑷锛堜笉闇€瑕?WinIsland 杩愯锛夛細

```powershell
cargo run --example host-smoke -- target/release/island_voice.dll
```

瀹冧細鍔犺浇 DLL锛岃皟鐢?`create 鈫?shutdown 鈫?destroy`锛屽苟妯℃嫙 Settings/Store/Log/Command/Context 鏈嶅姟琛ㄣ€?
> 鏈彃浠舵槸 `cdylib`锛孌LL 鍗虫彃浠舵湰浣擄紱涓嶉渶瑕?`.lib` 鎴栫嫭绔?exe銆?
## 3. 鎵撳寘鎴?WinIsland 鎻掍欢 ZIP

浠撳簱鎻愪緵 `package.rs`锛屼娇鐢ㄥ畼鏂?`PluginPackager`锛?
```powershell
cargo run --example pack
```

杈撳嚭锛?
```text
target/island-voice-1.1.2.zip
```

ZIP 鍐呭锛?
```text
plugin.yml
island_voice.dll
icon.png
README.md锛堝瓨鍦ㄦ椂锛?```

`plugin.yml` 鐢卞畼鏂?packager 鑷姩鐢熸垚锛屽寘鍚?DLL SHA-256銆丄BI 鐗堟湰鍜屽厓鏁版嵁锛涗笉瑕佹墜鏀瑰搱甯屻€?
### 鎵撳寘鍘熺悊涓庢敞鎰忎簨椤?
`PluginPackager::from_cargo()` 浼氾細

1. 璇诲彇 `Cargo.toml` 鐨?`name / version / authors / description / repository`锛?2. 杩愯 `cargo build --release --locked`锛?3. 鏍￠獙 DLL 瀵煎嚭鐨?`winisland_plugin_entry_v2` 涓庢弿杩扮鍏冩暟鎹紱
4. 鐢熸垚 `plugin.yml` 骞?zip銆?
鍥犳锛?
- 蹇呴』鎻愪氦/淇濈暀 `Cargo.lock`锛涚己澶辨椂鍏堣繍琛?`cargo generate-lockfile`銆?- `Cargo.toml` 鐨?`[package.metadata.winisland] id/name` 蹇呴』涓?`src/lib.rs` 鐨?`PluginMetadataC::new(...)` 瀹屽叏涓€鑷达細
  - id: `island-voice`
  - name: `IslandVoice`
  - version: `1.1.2`
  - author: `IslandVoice Contributors`
- 鏀规彃浠跺悕/浣滆€?浠撳簱鍦板潃鏃讹紝涓夊涓€璧锋敼锛歚Cargo.toml`銆乣src/lib.rs` 鎻忚堪绗︺€佹墦鍖呭厓鏁版嵁銆?- 瀹樻柟 packager 榛樿杈撳嚭 `target/<name>-<version>.zip`锛沗package.rs` 鏄惧紡鎸囧畾涓?`target/island-voice-<version>.zip`銆?
### 鍙€夛細绛惧悕

`PluginPackager` 鏀寔 Ed25519 绛惧悕锛?
```rust
PluginPackager::from_cargo()?
    .signing_key_path("signing_key.pem")
    .build()?;
```

鏈鍚嶆彃浠跺湪 WinIsland 涓€氬父闇€瑕佺敤鎴锋墜鍔ㄧ‘璁や俊浠伙紱涓汉浣跨敤涓嶅奖鍝嶃€?
## 4. 鏈粨搴撶殑鏋勫缓鑴氭湰

```powershell
powershell -ExecutionPolicy Bypass -File scripts/build.ps1
```

绛変环浜庯細

```powershell
cargo test
cargo build --release --locked
cargo run --example pack
```

鍙墦鍖咃細

```powershell
powershell -ExecutionPolicy Bypass -File scripts/package.ps1
```

## 5. 甯歌缂栬瘧闂

### `LNK1104: 鏃犳硶鎵撳紑鏂囦欢 ...Temp\lnk....tmp`

涓存椂鐩綍鏉冮檺/鏉€杞嫤鎴€傛妸涓存椂鐩綍鎸囧埌宸ヤ綔鍖猴細

```powershell
$env:TMP = "$PWD\.tmp"
$env:TEMP = "$PWD\.tmp"
New-Item -ItemType Directory -Force $env:TMP | Out-Null
cargo build --release
```

### `link.exe not found`

瀹夎 Visual Studio Build Tools 鐨?C++ 宸ヤ綔璐熻浇锛屾垨浣跨敤 鈥渪64 Native Tools Command Prompt鈥濄€?
### `failed to download from crates.io`

閰嶇疆绗?1 鑺傜殑 rsproxy 闀滃儚锛屾垨璁剧疆 `HTTP_PROXY/HTTPS_PROXY` 鍚庨噸璇曘€?
### `cargo build --release --locked` 澶辫触

`Cargo.lock` 涓?`Cargo.toml` 涓嶅悓姝ャ€傛墽琛岋細

```powershell
cargo update
cargo build --release
```

鍐嶉噸鏂?`cargo run --example pack`銆?
### 鎵撳寘鏃舵彁绀?`Plugin metadata mismatch`

`Cargo.toml` 鐨?`authors/version/description` 涓?`src/lib.rs` 鎻忚堪绗︿笉涓€鑷淬€傞€愰」瀵圭収淇敼銆?
### 棣栨缂栬瘧寰堟參

`reqwest + rustls + cpal + windows-sys` 棣栨缂栬瘧绾?1鈥? 鍒嗛挓锛況elease 寮€浜?LTO锛岀害 1鈥? 鍒嗛挓锛涗箣鍚庡閲忓緢蹇€?


