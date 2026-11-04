# 鏋舵瀯璇存槑

## 1. 杩愯褰㈡€?
IslandVoice 鏄?WinIsland 杩涚▼鍐呯殑鍘熺敓 `.dll`锛岄€氳繃 ABI v2 瀵煎嚭锛?
```rust
#[no_mangle]
pub unsafe extern "C" fn winisland_plugin_entry_v2() -> *const PluginDescriptorV2
```

鎻忚堪绗﹀０鏄庯細

- ABI锛歚ABI_VERSION_2`
- 鑳藉姏锛歚CAP_CONTEXT | CAP_SETTINGS | CAP_STORE | CAP_COMMAND`
- 鐢熷懡鍛ㄦ湡锛歚create` / `shutdown` / `destroy`
- 鍏冩暟鎹細`island-voice` / `IslandVoice` / `1.1.3` / `IslandVoice Contributors`
- 鏃?`on_tick`锛堟墍鏈夊伐浣滃湪绾跨▼涓畬鎴愶紝涓嶄緷璧栨覆鏌?tick锛?
## 2. 绾跨▼妯″瀷

| 绾跨▼ | 鑱岃矗 | 鍋滄鏂瑰紡 |
| --- | --- | --- |
| WinIsland 璋冪敤绾跨▼ | `create` / `shutdown` / `destroy` | host 椹卞姩 |
| manager | 涓茶澶勭悊璁剧疆鍙樻洿銆侀厤缃寔涔呭寲銆佸紩鎿庡惎鍋溿€丏eepSeek 娴嬭瘯 | `ConfigCommand::Shutdown` |
| audio | 鎵撳紑 cpal 杈撳叆娴侊紱鍥炶皟鍙仛鏍煎紡杞崲鍜屽叆闃?| `stop` + drop stream |
| processor | 閲嶉噰鏍枫€乂AD銆乂osk 鍞ら啋銆佸綍闊虫柇鍙?| 閫氶亾鏂紑 / `stop` |
| pipeline | STT 鈫?DeepSeek 鈫?涓婂矝 | 閫氶亾鏂紑 / `stop` |
| config-ui | 鍘熺敓閰嶇疆绐楀彛娑堟伅寰幆 | `WM_CLOSE`锛宍ConfigUi::close` join |

`shutdown` 椤哄簭锛?
1. 缃?`Shared.shutdown`
2. `ConfigUi::close()`锛堝叧闂厤缃獥鍙ｏ級
3. 鍚?manager 鍙戦€?`Shutdown`锛宮anager 鍋滄闊抽/绠￠亾骞堕€€鍑?4. 鍐嶆 `stop_engine()`锛堝箓绛夛級
5. 閲婃斁鍛戒护鍥炶皟銆佽缃〉銆侀€氱煡 Context
6. 杩斿洖 `PluginStatus::Ok`锛涢殢鍚?host 璋?`destroy`锛岄噴鏀惧疄渚?
杩欐弧瓒?SDK 鈥渟hutdown 蹇呴』鍋滄骞?join 鎻掍欢鑷湁绾跨▼鈥濈殑瑕佹眰銆?
## 3. 闊抽閾捐矾

```text
cpal default input (f32/i16/u16/i32/f64/i8/u8)
  鈫?澶氬０閬撳钩鍧?  鈫?绾挎€ч噸閲囨牱鍒?16 kHz
  鈫?20 ms 甯?  鈫?鑷€傚簲鑳介噺 VAD + Vosk
```

- 闊抽鍥炶皟涓笉鍋?Vosk/缃戠粶/閿佸ぇ鎿嶄綔锛屽彧 `try_send(Vec<f32>)`锛涢槦鍒楁弧鍒欎涪甯с€?- 澶勭悊鍣ㄦ瘡 100 ms 鍚?Vosk 鍠?1600 涓?i16 鏍锋湰锛沺artial/final 閮藉弬涓庡敜閱掑尮閰嶃€?- 鍞ら啋鍖归厤锛氬綊涓€鍖栧悗绮剧‘鍖呭惈锛屾垨瀵归暱搴︽帴杩戠殑绐楀彛鍋?Levenshtein 鐩镐技搴?鈮?0.8锛堟渶灏?4 瀛楃锛夈€?- 鍞ら啋鍚庤繘鍏ュ綍闊筹細鏈€闀?15 绉掞紝闈欓煶 900 ms 鏂彞锛? 绉掓湭妫€娴嬪埌璇煶鍒欏彇娑堛€?- 鎸囦护缁撴潫鍙戦€?`Utterance { pcm }` 缁?pipeline锛涘綍闊崇嚎绋嬬户缁洃鍚紝涓嶄細闃诲銆?
## 4. STT / LLM

- `stt::transcribe` 鏍规嵁 `SttMode` 閫夋嫨锛?  - Online锛歄penAI 鍏煎 multipart 涓婁紶锛?  - Offline锛歏osk 鏁存璇嗗埆锛?  - Auto锛氭湁 STT 瀵嗛挜鍒欏湪绾匡紝澶辫触鍥為€€绂荤嚎銆?- 鐢熸垚鐨?WAV 涓?16 kHz / 16-bit / mono / 44 瀛楄妭澶淬€?- DeepSeek锛歚POST {base}/chat/completions`锛宍stream=false`锛屽甫鏈€杩戝嚑杞巻鍙诧紱绯荤粺鎻愮ず闄愬埗鍥炵瓟闀垮害鍜屾牸寮忋€?- 浠ｇ悊閫氳繃 `reqwest::Proxy::all` 鍚屾椂浣滅敤浜庝袱涓鎴风銆?
## 5. 閰嶇疆涓庤缃〉

### 瀹樻柟璁剧疆 API 鐨勮兘鍔涜竟鐣?
`SettingsApiV2` 鐨?item kinds锛?
```text
SECTION / GROUP_START / GROUP_END / LABEL / SWITCH / SELECT / STEPPER / BUTTON
```

娌℃湁鏂囨湰妗嗐€侷slandVoice 鐨勫鐞嗭細

- 渚ф爮椤靛睍绀哄瓧娈靛綋鍓嶅€硷紙`BUTTON` 琛岀殑 value锛夊拰涓や釜 `SWITCH`锛?- 鐐瑰嚮 `field.*` 鎸夐挳 鈫?manager 鍦ㄧ嫭绔嬬嚎绋嬫墦寮€ Win32 閰嶇疆绐楀彛锛?- 绐楀彛淇濆瓨 鈫?閫氳繃 channel 鎶婂畬鏁?`Config` 浜ょ粰 manager锛?- manager 璋?`StoreApi::set` 鎸佷箙鍖栥€佸埛鏂拌缃〉銆佸繀瑕佹椂閲嶅惎寮曟搸銆?
璁剧疆椤靛洖璋?`on_change` 鍙仛 `try_send`锛屼笉鐩存帴璋冪敤 host API锛岄伩鍏嶅洖璋冮噸鍏?姝婚攣銆傝缃〉 `callback_data` 鎸囧悜绋冲畾鐨?`Box<SettingsCallback>`锛岄噴鏀炬椂鑻?host release 澶辫触鍒欐晠鎰忔硠婕忓洖璋冨唴瀛橈紙涓?SDK 鐨勫鐞嗕竴鑷达級锛岄伩鍏嶆偓鍨傛寚閽堛€?
### 鍘熺敓閰嶇疆绐楀彛

- 绾?Win32锛坄CreateWindowExW` + STATIC/EDIT/BUTTON/COMBOBOX锛夈€?- 鐙珛绾跨▼ + 娑堟伅寰幆锛沗GWLP_USERDATA` 淇濆瓨 `WindowState`銆?- 鍗曞疄渚嬶細宸叉湁绐楀彛鏃?`SetForegroundWindow` + `WM_APP_OPEN`銆?- 鍏抽棴/鍗歌浇锛歚PostMessage(WM_CLOSE)` + join銆?- 瀵嗙爜妗嗕娇鐢?`ES_PASSWORD`锛涗繚瀛樻椂鏍￠獙浠ｇ悊 scheme銆佸姪鎵嬪悕銆佹ā鍨嬪悕銆佸湪绾?STT 鍦板潃銆?
## 6. 鐏靛姩宀涢€氱煡

浣跨敤 `ContextApiV2::create`锛?
```rust
ContextDataV2 {
  priority: PRIORITY_HIGH,
  flags: CONTEXT_FLAG_SHOW_COMPACT,
  timeout_ms: 20_000,
  title: 鍔╂墜鍚?
  body: AI 鍥炲,
  compact_text: 鍓?44 瀛楃,
}
```

- 鍚屼竴鏃跺埢鍙繚鐣欎竴涓?Context锛涙柊鍐呭鍏堥噴鏀炬棫鐨勩€?- 鑷姩杩囨湡鐢?host 澶勭悊锛涢噴鏀炬棫 handle 鏃堕亣鍒?`StaleHandle` 蹇界暐銆?- 杩欏氨鏄€滄帹閫佹秷鎭埌鐏靛姩宀涜兌鍥娾€濆拰鈥滆皟鐢?WinIsland 閫氱煡鎻愮ず鈥濈殑缁熶竴瀹炵幇銆?
## 7. Vosk 鍔ㄦ€佸姞杞?
- 涓嶉摼鎺?`libvosk.lib`锛岃繍琛屾椂鐢?`LoadLibraryExW(LOAD_WITH_ALTERED_SEARCH_PATH)` 鍔犺浇锛屼繚璇佷緷璧?DLL 浠庡悓鐩綍瑙ｆ瀽銆?- 瑙ｆ瀽瀵煎嚭锛歚vosk_model_new/free`銆乣vosk_recognizer_new/free`銆乣accept_waveform`銆乣result/partial_result/final_result`銆乣set_log_level`銆?- 妯″瀷/杩愯鏃舵悳绱㈤『搴忥細
  1. 閰嶇疆闈㈡澘鏄惧紡璺緞
  2. `%APPDATA%\WinIsland\IslandVoice\vosk\libvosk.dll`
  3. `%APPDATA%\WinIsland\IslandVoice\libvosk.dll`
  4. 鎻掍欢 DLL 鎵€鍦ㄧ洰褰?`runtime\`銆佹牴鐩綍
  5. `models\` 涓嬬殑妯″瀷鐩綍锛堣嚜鍔ㄦ寜鍞ら啋璇嶈瑷€鎺掑簭锛?- 鎵句笉鍒版椂涓嶉樆姝㈡彃浠跺姞杞斤細鍞ら啋涓嶅彲鐢紝浠嶅彲鈥滅珛鍗宠璇濃€濓紱绂荤嚎 STT 涓嶅彲鐢ㄦ椂鍦ㄧ嚎浠嶅彲鐢ㄣ€?
## 8. 閿欒澶勭悊

- 鎵€鏈夎法 FFI 鐨勫叆鍙ｇ敤 `catch_unwind` 鍖呰９锛宲anic 杞?`PluginStatus::Internal`銆?- 闊抽/Vosk/缃戠粶閿欒閫氳繃 `Shared::set_error` 鍚屾椂鍐?host 鏃ュ織鍜岀伒鍔ㄥ矝鎻愮ず銆?- 閰嶇疆淇濆瓨澶辫触浼氭彁绀猴紝浣嗗唴瀛橀厤缃粛鐢熸晥銆?- 鍏抽棴绐楀彛/鍋滄寮曟搸骞傜瓑锛沗stop_engine` 鍙噸澶嶈皟鐢ㄣ€?
## 9. 宸茬煡杈圭晫

- Context 姝ｆ枃 511 瀛楄妭锛岄暱鍥炲鎴柇銆?- 鍦ㄧ嚎 HTTP 鍚屾璇锋眰锛屾彃浠跺嵏杞芥渶澶氱瓑寰呰姹傝秴鏃躲€?- Vosk 灏忓瀷妯″瀷瀵圭敓鍍昏瘝/鍙ｉ煶鏈夐檺锛涘彲鍦ㄨ嚜妫€鑴氭湰涓‘璁ゅ疄闄呰浆鍐欍€?- 涓嶅疄鐜板伐鍏疯皟鐢ㄣ€佸杞闊虫墦鏂€佹祦寮?TTS銆?


