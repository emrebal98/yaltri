# Yaltri

*Yaltrı-* (Göktürkçe): parıldamak. Sistem tepsisinde yaşayan, kişisel masaüstü araçları.
Windows, Ubuntu ve Arch üzerinde aynı görünüm için [GPUI](https://gpui.rs) ile yazıldı.

| Araç | Durum |
|---|---|
| Launcher: hesap makinesi + birim çevirme | ✅ |
| Launcher: uygulama arama (Start Menu + Store / `.desktop`) | ✅ |
| Launcher: dosya/klasör arama (kendi index'imiz, Everything gerekmez) | ✅ |
| Renk seçici (büyüteç, 8 format, geçmiş, tonlar) | ✅ |

## Kullanım

```sh
yaltri                    # arka planda başlat (tray ikonu)
yaltri toggle launcher    # arama penceresini aç/kapat
yaltri settings           # ayarlar
yaltri quit
```

Launcher: `↑/↓` seç, `Enter` aç (hesap sonucunu kopyalar), `Ctrl+Enter` klasörde göster, `Esc` kapat.
Örnek girdiler: `2^10`, `5 km to mi`, `=pi`, `code`, `cargo toml`. Sık açılanlar üste çıkar.

### Renk seçici

`yaltri toggle picker` (varsayılan kısayol `Win+Shift+C`; PowerToys açıksa çakışır, ayarlardan değiştirin).
İmleç yanında büyüteç ve renk kodu görünür: tekerlek yakınlaştırır, sol tık seçer ve panoya kopyalar,
sağ tık / `Esc` iptal eder. Seçimden sonra editör açılır: geçmiş (sağ tık siler), açık/koyu tonlar,
HEX · RGB · HSL · HSV · CMYK · HWB · Decimal · Float. ★ ile seçimde kopyalanacak format belirlenir.

- Windows / X11: masaüstü görüntüsü alınır, her ekranda dondurulmuş haliyle gösterilir.
- Wayland (GNOME, KDE, Hyprland): ekran okunamadığı için masaüstünün kendi seçicisi
  (`xdg-desktop-portal` PickColor) açılır, sonuç aynı editöre gelir.

### Dosya index'i

Everything gibi sadece isimleri RAM'de tutar: ~1,9M dosya ≈ 210 MiB, arama ~8 ms.
İlk tarama tüm çekirdeklerle birkaç saniye–dakika sürer; index diske kaydedilir ve sonraki
açılışta ~0,1 sn'de yüklenir. Sonraki taramalar index'i yeniden kurmaz, diskle karşılaştırıp
sadece farkları uygular (arka planda 4 thread, tepe bellek ~+35 MiB).

- Windows: tüm sabit diskler taranır, değişiklikler anlık izlenir (`ReadDirectoryChangesW`, yönetici yetkisi gerekmez).
- Linux: `/` taranır; sadece ev dizini anlık izlenir (inotify limiti), gerisi saatlik yeniden taranır.
- Hariç tutulanlar: `C:\Windows`, `/proc`, `/sys` vb. sistem klasörleri, `node_modules`, `.git`.
  AppData, nokta-klasörler ve önbellekler index'lenir ama sonuçlarda alta itilir.
- `settings.toml` → `[files]`: `roots`, `exclude_paths`, `exclude_names`, `files_enabled`, `apps_enabled`.

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
  yaltri-files/     dosya index'i: paralel tarama, kompakt isim havuzu, snapshot, değişiklik izleme
  yaltri-apps/      uygulama keşfi (Start Menu/Get-StartApps, .desktop) ve eşleştirme
  yaltri-color/     renk dönüşümleri, formatlar, tonlar, renk geçmişi
  yaltri-platform/  tray, global kısayol, tek-instance IPC, oturum tespiti, program başlatma
  yaltri/           GPUI uygulaması: daemon, launcher, renk seçici + editör, ayarlar penceresi
```

GPUI, `gpui-kit = "=0.9.1"` ile tam sürüme sabitlenmiştir; GPUI 1.0 öncesi olduğundan güncellemeler bilinçli yapılmalı.
