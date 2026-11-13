# Yaltri

*Yaltrı-* (Göktürkçe): parıldamak. Sistem tepsisinde yaşayan, kişisel masaüstü araçları.
Windows, Ubuntu ve Arch üzerinde aynı görünüm için [GPUI](https://gpui.rs) ile yazıldı.

| Araç | Durum |
|---|---|
| Arama / launcher (hesap makinesi + birim çevirme) | ✅ |
| Dosya arama (Windows: Everything, Linux: kendi index) | planlandı |
| Renk seçici | planlandı |

## Kullanım

```sh
yaltri                    # arka planda başlat (tray ikonu)
yaltri toggle launcher    # arama penceresini aç/kapat
yaltri settings           # ayarlar
yaltri quit
```

Launcher: `↑/↓` seç, `Enter` uygula (hesap sonucunu kopyalar), `Esc` kapat.
Örnek girdiler: `2^10`, `sqrt(2)`, `5 km to mi`, `0xff to decimal`, `=pi`.

Ayarlar: `%APPDATA%\yaltri\config\settings.toml` / `~/.config/yaltri/settings.toml`.
Log: `%LOCALAPPDATA%\yaltri\data\logs` / `~/.local/share/yaltri/logs` (`YALTRI_LOG=debug`).

### Wayland

Wayland uygulamaların global kısayol almasına izin vermez. Kısayolu masaüstü ayarlarından bağlayın:

- **GNOME (Ubuntu):** Ayarlar → Klavye → Klavye Kısayolları → Özel Kısayollar → komut: `yaltri toggle launcher`
- **KDE:** Sistem Ayarları → Kısayollar → Özel Kısayollar
- **Hyprland:** `bind = SUPER, SPACE, exec, yaltri toggle launcher`

GNOME'da launcher XWayland üzerinden açılır (Ulauncher ile aynı yöntem). KDE/Hyprland'de layer-shell kullanılır.
Tray ikonu GNOME'da AppIndicator eklentisi ister (Ubuntu'da varsayılan olarak yüklü).

## Derleme

```sh
cargo run -p yaltri
cargo test --workspace
```

Linux'ta GPUI sistem kütüphaneleri ister (Wayland, X11/xcb, xkbcommon, fontconfig, Vulkan).
Güncel liste için Zed'in `script/linux` betiğine bakın; Ubuntu için kabaca:

```sh
sudo apt install build-essential pkg-config libxkbcommon-dev libxkbcommon-x11-dev \
  libwayland-dev libx11-xcb-dev libxcb1-dev libfontconfig-dev libfreetype-dev libvulkan1
```

## Yapı

```
crates/
  yaltri-core/      ayarlar, komutlar, tool id'leri, arama sözleşmesi (UI'dan bağımsız)
  yaltri-calc/      fend-core tabanlı hesap sağlayıcısı
  yaltri-platform/  tray, global kısayol, tek-instance IPC, oturum tespiti
  yaltri/           GPUI uygulaması: daemon, launcher, ayarlar penceresi
```

GPUI, `gpui-kit = "=0.9.1"` ile tam sürüme sabitlenmiştir; GPUI 1.0 öncesi olduğundan güncellemeler bilinçli yapılmalı.
