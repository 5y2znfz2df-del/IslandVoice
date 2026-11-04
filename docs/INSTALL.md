# 瀹夎鎸囧崡

## 0. 鍓嶇疆

- WinIsland 宸插畨瑁呭苟鑳芥甯告樉绀虹伒鍔ㄥ矝锛圵indows 10 2004+ / 11 x64锛夈€?- 宸茶幏寰楁彃浠跺寘锛歚island-voice-1.1.3.zip`銆?- 璇煶鍔熻兘闇€瑕侀害鍏嬮锛沇indows 璁剧疆 鈫?闅愮 鈫?楹﹀厠椋?涓厑璁告闈㈠簲鐢ㄨ闂€?
## 1. 瀹夎 Vosk 绂荤嚎鍞ら啋杩愯鏃朵笌妯″瀷

IslandVoice 涓嶅己鍒朵緷璧栦粯璐瑰敜閱?SDK銆傚厤璐圭绾挎柟妗堜娇鐢?[Vosk](https://alphacephei.com/vosk/)锛圓pache-2.0锛夛細

- `libvosk.dll` 鍙婁緷璧栵紙Windows x64锛?- 涓€涓?Vosk 灏忔ā鍨?  - 鑻辨枃锛歚vosk-model-small-en-us-0.15`锛堢害 40 MB锛?  - 涓枃锛歚vosk-model-small-cn-0.22`锛堢害 42 MB锛?
### 鏂瑰紡 A锛氫竴閿剼鏈紙鎺ㄨ崘锛?
鍦ㄦ彃浠舵簮鐮佹牴鐩綍鎵ц锛?
```powershell
powershell -ExecutionPolicy Bypass -File scripts/fetch-vosk.ps1 -Language en
```

涓枃锛?
```powershell
powershell -ExecutionPolicy Bypass -File scripts/fetch-vosk.ps1 -Language zh
```

涓嫳鏂囬兘瑁咃細

```powershell
powershell -ExecutionPolicy Bypass -File scripts/fetch-vosk.ps1 -Language both
```

榛樿瀹夎鍒帮細

```text
%APPDATA%\WinIsland\IslandVoice\
鈹溾攢 vosk\
鈹? 鈹溾攢 libvosk.dll
鈹? 鈹溾攢 libgcc_s_seh-1.dll
鈹? 鈹溾攢 libstdc++-6.dll
鈹? 鈹斺攢 libwinpthread-1.dll
鈹斺攢 models\
   鈹斺攢 vosk-model-small-en-us-0.15\   锛堟垨 cn锛?```

鑴氭湰浼樺厛浠庡畼鏂?GitHub Release 涓嬭浇 Vosk 杩愯鏃讹紱澶辫触鏃跺皾璇?PyPI 鐨?`vosk` wheel锛堝悓鏍峰寘鍚?`libvosk.dll`锛夈€傛ā鍨嬩紭鍏?alphacephei锛屽け璐ユ椂鑻辨枃妯″瀷鍥為€€ HuggingFace 闀滃儚銆?
### 鏂瑰紡 B锛氭墜鍔ㄥ畨瑁?
1. 涓嬭浇 Vosk Windows 杩愯鏃讹細
   - https://github.com/alphacep/vosk-api/releases 锛坄vosk-win64-0.3.45.zip`锛?   - 鎴?PyPI `vosk` 鐨?win_amd64 wheel锛岃В鍘嬪悗 `vosk/` 鐩綍鍐呮湁 `libvosk.dll`
2. 鎶?`libvosk.dll`銆乣libgcc_s_seh-1.dll`銆乣libstdc++-6.dll`銆乣libwinpthread-1.dll` 鏀惧埌
   `%APPDATA%\WinIsland\IslandVoice\vosk\`銆?3. 涓嬭浇妯″瀷锛?   - 鑻辨枃锛歨ttps://alphacephei.com/vosk/models/vosk-model-small-en-us-0.15.zip
   - 涓枃锛歨ttps://alphacephei.com/vosk/models/vosk-model-small-cn-0.22.zip
4. 瑙ｅ帇鍒?`%APPDATA%\WinIsland\IslandVoice\models\`锛岀‘淇濈洰褰曠粨鏋勫寘鍚細

```text
models\vosk-model-small-en-us-0.15\am\final.mdl
models\vosk-model-small-en-us-0.15\conf\
models\vosk-model-small-en-us-0.15\graph\
```

> 鎻掍欢涔熶細鍦ㄥ綋鍓?DLL 鐩綍鐨?`runtime\`銆乣models\` 瀛愮洰褰曚腑鏌ユ壘锛涙墦鍖呮椂鎶婅繍琛屾椂鏀惧湪鎻掍欢婧愮爜鐨?`runtime\` 鎴?`models\` 鐩綍锛宍package.rs` 浼氳嚜鍔ㄤ竴璧锋墦鍖呫€傚崌绾ф彃浠舵椂杩欎簺鏂囦欢浼氶殢鍖呮浛鎹紱鎺ㄨ崘浠嶇敤 `%APPDATA%` 璺緞锛屽崌绾ф彃浠朵笉浼氫涪銆?
## 2. Edge 绁炵粡璇煶锛堟帹鑽愶紝鍙€夛級

IslandVoice 榛樿鐢ㄥ井杞?Edge 鐨勩€屾檽鏅撱€嶇缁忚闊虫湕璇诲洖澶嶏紝闊宠壊姣?Windows 鑷甫 SAPI 鏇磋嚜鐒躲€傚畠涓嶉渶瑕?API Key锛屼絾闇€瑕侊細

1. Python 3.9+
2. `edge-tts` 鍖?
```powershell
powershell -ExecutionPolicy Bypass -File scripts/install-edge-tts.ps1
```

鎴栨墜鍔細

```powershell
python -m pip install --upgrade edge-tts
```

鎻掍欢浼氫緷娆″皾璇曠粷瀵硅矾寰?Python銆乣py -3.13`銆乣python`銆乣py -3` 璋冪敤 `python -m edge_tts`锛涘鏋滈兘娌¤鎴栨柇缃戯紝浼氳嚜鍔ㄩ€€鍥?Windows SAPI 绂荤嚎璇煶锛屼笉褰卞搷鍔熻兘銆?
## 3. 涓枃鍚啓锛堥粯璁わ級

涓枃瀵硅瘽涓嶉渶瑕侀澶栦笅杞芥ā鍨嬶細IslandVoice 浼氭妸褰曢煶浜ょ粰 Windows 鑷甫鐨?`zh-CN` SAPI 鍚啓鍣紙`System.Speech.Recognition`锛夈€傜郴缁熼渶瀹夎涓枃璇煶璇嗗埆缁勪欢锛堜腑鏂?Windows 榛樿宸叉湁锛夈€?
鍞ら啋璇嶄粛鏄?`hey siri`锛涘敜閱掑悗鐩存帴鐢ㄤ腑鏂囪鎸囦护鍗冲彲銆傝嫢瑕佷腑鏂囧敜閱掕瘝锛岄渶瑕侀澶栧畨瑁呬腑鏂?Vosk 妯″瀷骞惰皟鏁村敜閱掕瘝銆?
## 4. 楠岃瘉 Vosk 鏄惁鍙敤

```powershell
powershell -ExecutionPolicy Bypass -File scripts/verify-vosk.ps1 -Language en
```

鑴氭湰浼氱敤 Windows SAPI 鐢熸垚 16 kHz 鍗曞０閬?`hey siri` WAV锛岃皟鐢?`examples/vosk-check`锛屾湡鏈涜緭鍑猴細

```text
transcript: hey siri
wake detected: hey siri
```

濡傛灉澶辫触锛岃 [TROUBLESHOOTING.md](TROUBLESHOOTING.md#vosk-鐩稿叧)銆?
## 2. 瀹夎鎻掍欢

1. 鎵撳紑 WinIsland 璁剧疆锛堟墭鐩樺弻鍑?/ 鍙抽敭鑿滃崟锛夈€?2. 杩涘叆鈥滄彃浠垛€濋〉锛岄€夋嫨鈥滀粠鏈湴鏂囦欢瀹夎 / 瀹夎鎻掍欢鍖呪€濄€?3. 閫夋嫨 `target/island-voice-1.1.3.zip`銆?4. 鍦ㄦ彃浠跺垪琛ㄤ腑鍚敤 **IslandVoice**銆?
涓嶅悓 WinIsland 鐗堟湰鐨勫叆鍙ｅ悕绉板彲鑳界暐鏈夊樊寮傦紱鏍稿績鏄€夋嫨鏈湴 `.zip`锛堜笉鏄彧閫?`.dll`锛夈€?
## 3. 棣栨閰嶇疆

1. 鎵撳紑 WinIsland 璁剧疆 鈫?宸︿晶 **IslandVoice**銆?2. 鐐瑰嚮鈥滃畬鏁撮厤缃潰鏉库€濇垨浠绘剰瀛楁琛岋紝鎵撳紑鍘熺敓閰嶇疆绐楀彛銆?3. 濉啓锛?   - 鍔╂墜鍚嶇О锛氫緥濡?`灏忓矝`
   - 鍞ら啋璇嶏細渚嬪 `hey siri`
   - API 瀵嗛挜锛歚sk-...`锛圖eepSeek 鎺у埗鍙板垱寤猴級
   - 妯″瀷鍚嶇О锛歚deepseek-chat`
   - HTTP 浠ｇ悊锛氬彲鐣欑┖锛涘浗鍐呯洿杩?DeepSeek 閫氬父鍙敤
   - 鑷姩鐩戝惉 / 鐏靛姩宀涙樉绀哄洖澶嶏細鎸夐渶鍕鹃€?4. 淇濆瓨銆?5. 鐐瑰嚮璁剧疆椤碘€淒eepSeek 杩炴帴娴嬭瘯鈥濋獙璇佸瘑閽ヤ笌缃戠粶銆?6. 鐐瑰嚮鈥滅珛鍗宠璇濃€濇垨璇村敜閱掕瘝寮€濮嬨€?
## 4. 閰嶇疆淇濆瓨浣嶇疆

IslandVoice 浣跨敤 WinIsland 瀹樻柟 `StoreApi`锛岄厤缃繚瀛樺湪 WinIsland 鐨勬彃浠舵暟鎹懡鍚嶇┖闂翠腑锛岄殢鎻掍欢鍗歌浇/閲嶈绛栫暐鐢?WinIsland 绠＄悊銆侷slandVoice 涓嶉澶栧垱寤洪厤缃枃浠躲€?
Vosk 杩愯鏃跺拰妯″瀷鏄閮ㄦ枃浠讹紝浣嶄簬 `%APPDATA%\WinIsland\IslandVoice\`锛屽嵏杞芥彃浠朵笉浼氬垹闄わ紱闇€瑕佸交搴曟竻鐞嗘椂鎵嬪姩鍒犻櫎璇ョ洰褰曘€?
## 5. 鍗歌浇

1. WinIsland 璁剧疆 鈫?鎻掍欢 鈫?鍋滅敤/鍗歌浇 IslandVoice銆?2. 鍙€夛細鍒犻櫎 `%APPDATA%\WinIsland\IslandVoice\`锛堜細涓€骞跺垹闄?Vosk 妯″瀷锛夈€?3. 鎻掍欢涓嶄細淇敼绯荤粺娉ㄥ唽琛ㄣ€佷笉浼氳缃紑鏈鸿嚜鍚」锛堢洃鍚紑鍏崇敱 WinIsland 鐨勬彃浠剁敓鍛藉懆鏈熺鐞嗭級銆?
## 6. 鍗囩骇

1. 鐢ㄦ柊鐗堟湰 zip 鍦?WinIsland 涓鐩栧畨瑁?鍗歌浇鍚庨噸瑁呫€?2. 閰嶇疆瀛樹簬 WinIsland Store锛岄€氬父淇濈暀锛涘鏈繚鐣欙紝閲嶆柊濉啓鍗冲彲銆?3. Vosk 杩愯鏃跺彲澶嶇敤锛屼笉蹇呴噸澶嶄笅杞姐€?


