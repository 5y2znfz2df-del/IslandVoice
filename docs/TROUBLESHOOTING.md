# 排障

## 插件相关

### 设置里看不到 IslandVoice

- 确认 zip 安装成功且插件已启用；只复制 `.dll` 不会被识别，必须走插件包。
- 查看 WinIsland 插件日志；`create` 失败通常返回 `UnsupportedVersion` 或 `Internal`。
- 确认 WinIsland 版本支持 ABI v2（`winisland-plugin-api` 0.8+，建议 0.11 对应的最新版）。

### 插件加载后立刻被卸载/报错

- `plugin.yml` 的 `entry` 与实际 DLL 文件名不一致。
- DLL 描述符元数据与 `plugin.yml` 不一致（id/name/version/author）。
- 重新用 `cargo run --example pack` 生成，不要手改哈希。

### 设置页没有“完整配置面板”入口

- 侧栏页面由 `SettingsApiV2` 创建；若 host 不支持设置页，会记录“无法创建设置页”。
- 仍可通过快捷键/托盘命令使用，但无法改配置；请确认 WinIsland 为支持插件的版本。

### 点击字段没有弹出配置窗口

- 看是否有其他 IslandVoice 配置窗口已打开（单实例，会聚焦已有窗口）。
- 检查 WinIsland 日志中的 “创建配置管理线程失败”。
- 若窗口在屏幕外（多显示器变化），关闭插件重开，或重启 WinIsland。

### 卸载/退出 WinIsland 卡住

- 插件正在等待网络请求（DeepSeek 最长 30 秒，STT 最长 60 秒）。
- 先关闭配置窗口，等待请求结束；必要时结束 WinIsland 进程。
- 这是同步 HTTP 的已知边界，后续可改为可取消的异步请求。

## Vosk 相关

### 提示“未找到 libvosk.dll”

- 运行 `scripts/fetch-vosk.ps1`，或把 `libvosk.dll` 放到
  `%APPDATA%\WinIsland\IslandVoice\vosk\`。
- 确认是 **64 位** DLL（插件是 x64）。
- 错误 126/找不到依赖：`libgcc_s_seh-1.dll`、`libstdc++-6.dll`、`libwinpthread-1.dll` 必须与 `libvosk.dll` 同目录。
- 也可以在配置面板填写 `libvosk.dll` 绝对路径。

### 提示“未找到 Vosk 模型”

- 模型目录必须包含 `am\final.mdl` 或 `conf\model.conf`。
- 默认搜索 `%APPDATA%\WinIsland\IslandVoice\models\` 下的子目录。
- 也可以在配置面板填写模型目录绝对路径。

### 唤醒没反应

1. 先跑 `scripts/verify-vosk.ps1`，确认 Vosk 本身可用。
2. 看模型语言是否匹配唤醒词：
   - 英文唤醒词需要英文模型；
   - 中文唤醒词需要中文模型。
3. 换更常见的词（2–5 音节），避免生僻词。
4. 确认麦克风权限和默认输入设备；系统音量/麦克风静音。
5. 环境噪声太大时，VAD 可能把语音当噪声；靠近麦克风、降低噪声。
6. 某些耳机麦克风默认采样率异常，可在 Windows 声音设置中改为 16 kHz/48 kHz。

### 唤醒词能被识别但总是不触发

- 用 `verify-vosk.ps1` 看 Vosk 实际转写；如果差异大，换一个与转写接近的唤醒词。
- 匹配阈值是保守的编辑距离（≥0.8）；这是为了减少误唤醒。
- 背景音乐/影视声可能造成误触发或干扰。

### 离线转写为空/乱码

- 模型不完整（缺 `graph`、`conf`、`am`）。
- 音频不是 16 kHz 单声道（插件内部会重采样，但外部 WAV 自检需 16 kHz）。
- 说话太短/太轻，VAD 已取消录音。

## 网络 / DeepSeek

### “尚未填写 DeepSeek API 密钥”

在配置面板填写 `sk-...` 并保存。

### 401

密钥错误/过期；到 DeepSeek 控制台重新创建。注意不要带多余空格（保存时会 trim）。

### 402

账户余额不足。

### 429

请求过于频繁；等待后重试。连续多次唤醒会触发。

### 超时 / 连接失败

- 国内网络通常可直连 `api.deepseek.com`；不行就填 HTTP 代理。
- 代理格式：`http://127.0.0.1:7890`、`socks5://127.0.0.1:1080`。
- 先用“DeepSeek 连接测试”验证。
- 公司网络可能拦截 TLS；换网络/代理。

### 在线 STT 失败

- 检查 STT 地址是否包含 `/v1`（示例：`https://api.siliconflow.cn/v1`）。
- 检查 STT 密钥、模型名。
- 若服务不支持 `language` 字段，把识别语言改为 `auto`。
- 默认“自动”模式会回退到 Vosk；如果 Vosk 也未安装，会同时报两个错误。

### 灵动岛没有显示回复

- 配置中“灵动岛显示 AI 回复”是否关闭。
- 回复是否超时（20 秒后自动消失）。
- 检查是否被更高优先级的系统通知覆盖。
- 错误/识别结果仍应显示；完全不显示时看日志和 Context API 支持情况。

## 构建相关

见 [BUILD.md](BUILD.md#5-常见编译问题)。

## 收集日志

- WinIsland 插件日志（设置 → 关于/日志，或安装目录 logs）。
- IslandVoice 日志前缀：`[IslandVoice]`。
- 复现步骤 + 日志 + Windows 版本 + WinIsland 版本 + 是否安装 Vosk。
