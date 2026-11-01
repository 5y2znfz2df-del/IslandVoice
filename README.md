# IslandVoice 1.1.2

WinIsland 的语音 AI 助手插件：唤醒后说中文，DeepSeek 回答，灵动岛显示，并用 Edge 晓晓朗读。

## 功能

- 唤醒词 `hey siri`（英文）
- 唤醒后直接说中文，Windows 自带 zh-CN 离线听写
- 圆形 Siri 光球从灵动岛弹出，录音期间一直停留
- **说话后停顿约 5 秒**自动结束；唤醒后不说话等 15 秒；最长 60 秒
- DeepSeek 中文回答 + Edge 神经语音朗读（失败自动退回 SAPI）
- 快捷键：`Ctrl+Alt+V` 直接说话，`Ctrl+Alt+J` 开关监听

## 安装

1. 完全退出 WinIsland
2. 解压 `island-voice-1.1.2.zip` 到：

```text
%APPDATA%\WinIsland\plugins\island-voice\
```

确保 `plugin.yml` 直接在这个目录里。
3. 启动 WinIsland
4. 右键托盘 → IslandVoice：配置，填入自己的 DeepSeek API Key
5. 右键托盘 → IslandVoice：测试朗读

或者运行 `安装.ps1`（支持 `-WithVosk -WithEdgeTts`）。

## 依赖

- WinIsland 2.x / Windows 11 x64
- 自己的 DeepSeek API Key
- 中文识别：Windows zh-CN 语音识别组件（中文 Windows 默认有）
- `hey siri` 唤醒：Vosk 英文运行时 + 模型（可选，约 40 MB）
- 晓晓语音：Python 3 + `edge-tts`（可选，失败用 SAPI）

没有 Vosk 时，按 `Ctrl+Alt+V` 仍可中文对话。

## 隐私

- 麦克风持续本地监听唤醒词
- 唤醒后的识别文字发送给 DeepSeek
- Edge 朗读的回复文字发送给 Microsoft
- API Key 明文保存在本机 `store` 目录，不要分享

## 许可证

GPL-3.0-only，源码在 `source/`。
