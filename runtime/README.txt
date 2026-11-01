Optional Vosk runtime directory.

If you want to ship libvosk.dll inside the plugin package, copy the official
Windows runtime files here before packaging:

  libvosk.dll
  libgcc_s_seh-1.dll
  libstdc++-6.dll
  libwinpthread-1.dll

The plugin looks for:
  <plugin dir>\runtime\libvosk.dll
  <plugin dir>\libvosk.dll

Recommended installation instead:
  %APPDATA%\WinIsland\IslandVoice\vosk\libvosk.dll
  %APPDATA%\WinIsland\IslandVoice\models\<model>\
