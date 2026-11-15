<p align="center"><img src="design/brand/app-icon.svg" width="112" height="112" alt="Yaltri ikonu"></p>

<h1 align="center">Yaltri</h1>

<p align="center">
  <b>Sistem tepsisinde bekleyen küçük bir araç kutusu.</b><br>
  Kısayola bas, aracı kullan, işin bitince kaybolsun.
</p>

<p align="center">
  <a href="https://github.com/emrebal98/yaltri/releases"><img src="https://img.shields.io/github/v/release/emrebal98/yaltri?include_prereleases" alt="Release"></a>
  <a href="https://github.com/emrebal98/yaltri/actions/workflows/ci.yml"><img src="https://github.com/emrebal98/yaltri/actions/workflows/ci.yml/badge.svg" alt="CI"></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-GPL--3.0-blue.svg" alt="License: GPL-3.0"></a>
</p>

<p align="center"><a href="README.md">English</a> · <b>Türkçe</b></p>

Windows ve Linux'ta (Ubuntu, Arch) aynı görünür. Tek küçük bir programdır, yönetici izni istemez; internete yalnızca güncelleme denetlerken bağlanır.
*Yaltrı-* Göktürkçede "parıldamak" demek.

---

## Araçlar

### 🔎 Arama · `Super+Shift+F`

Uygulamalar, dosyalar ve oyunlar tek kutuda; içinde hesap makinesi de var: `2^10`, `5 km to mi`, `=pi`.
Yazdıkça sonuçlar gelir; `Enter` sonucu açar ya da cevabı kopyalar.

<p align="center"><img src="docs/media/launcher.gif" width="680" alt="Yaltri arama kutusu: 2^10 sonucu 1024, /a note ise Notepad, Sticky Notes ve OneNote'u listeler"></p>

### 🎨 Renk seçici · `Super+Shift+C`

Ekranın herhangi bir yerinden renk al, tam pikseli tutturmak için fare tekerleğiyle yakınlaş,
HEX, RGB, HSL, OKLCH ya da beş başka formatta kopyala.

<p align="center"><img src="docs/media/color-picker.gif" width="720" alt="Renk seçme: kod ve ad imleci izler, fare tekerleği bir köşede büyüteci açar, tıklayınca editör dokuz formatı ve geçmişi gösterir"></p>

Her araç **Ayarlar**'dan kapatılabilir.

---

## Kurulum

### Windows

[**yaltri-windows-x86_64.exe**](https://github.com/emrebal98/yaltri/releases/latest/download/yaltri-windows-x86_64.exe) dosyasını indir ve çalıştır.
Bu kadar: Yaltri kendini kullanıcın için kurar (yönetici izni gerekmez), Başlat menüsüne eklenir ve tepside belirir.
Güncellemek için yeni sürümü aynı şekilde indirip çalıştır.

> İlk açılışta Windows "bilinmeyen yayıncı" uyarısı gösterebilir: **Ek bilgi → Yine de çalıştır**.

### Linux

```sh
curl -fsSL https://github.com/emrebal98/yaltri/releases/latest/download/install.sh | sh
```

Script Yaltri'yi `~/.local/bin` içine koyar, başlatır ve eksik bir sistem kütüphanesi varsa kurulum komutunu söyler.
Güncellemek için tekrar çalıştır.

<details>
<summary>Elle kurulum</summary>

```sh
curl -LO https://github.com/emrebal98/yaltri/releases/latest/download/yaltri-linux-x86_64.tar.gz
tar xzf yaltri-linux-x86_64.tar.gz
install -Dm755 yaltri-linux-x86_64/yaltri ~/.local/bin/yaltri
yaltri
```

</details>

Eksik kütüphane hatası alırsan:

```sh
sudo apt install libxkbcommon-x11-0 libvulkan1 mesa-vulkan-drivers   # Ubuntu
sudo pacman -S libxkbcommon-x11 vulkan-icd-loader                    # Arch
```

GNOME'da tepsi ikonu için AppIndicator eklentisi gerekir (Ubuntu'da hazır gelir).
Yaltri kendini uygulama menüsüne ekler.

Yaltri ilk açılıştan itibaren sistemle birlikte başlar; **Ayarlar → Genel → Sistem açılışında başlat**'tan kapatabilirsin.

---

## Kullanım

| Kısayol | Ne yapar |
|---|---|
| `Super+Shift+F` | Arama kutusunu açar |
| `Super+Shift+C` | Renk seçiciyi açar |

Kısayolları **Ayarlar** penceresinden değiştirebilirsin (tepsi ikonuna sağ tık → Ayarlar).

### Arama

| Tuş | |
|---|---|
| `↑` `↓` | Sonuçlar arasında gez |
| `Enter` | Aç |
| `Ctrl+Enter` | Dosyanın bulunduğu klasörü aç |
| `Tab` / `Shift+Tab` | Filtre değiştir: Tümü, Uygulamalar, Dosyalar, Oyunlar |
| `Backspace` (kutu boşken) | Filtreyi kaldır |
| `Esc` | Kapat |

Yazarken filtrelemek için başa kısaltma ve boşluk yaz: `/u ` uygulama, `/d ` dosya, `/o ` oyun
(`/a`, `/f`, `/g` de çalışır). Filtre açıkken kutu boşsa o türdeki her şey listelenir, örneğin bütün oyunların.

- **Uygulamalar:** Başlat menüsündeki ve mağazadan kurulan uygulamalar, Linux'ta kurulu masaüstü uygulamaları.
- **Dosyalar:** Yaltri bilgisayarındaki dosya adlarını kendi index'inde tutar, milyonlarca dosyada bile anında bulur.
  İlk tarama birkaç dakika sürebilir, sonra değişiklikleri kendisi takip eder.
  Linux'ta çok fazla klasör içeren ev klasörleri canlı izlenmez, saatte bir yeniden taranır;
  böylece Yaltri diğer programların da ihtiyaç duyduğu sistem dosya izleme limitini tüketmez.
- **Oyunlar:** Steam, Xbox / PC Game Pass, Epic Games, Riot Games, GOG, Ubisoft Connect, EA app;
  Linux'ta Steam, Heroic ve Lutris. Oyunlar kendi launcher'ları üzerinden açılır.
- **Hesap:** Matematik, birim çevirme ve sabitler. Sık kullandıkların listede üste çıkar.

### Renk seçici

- İmlecin yanında rengin kodu ve adı görünür.
- **Fare tekerleği** ile büyüteci aç, yakınlaştır, uzaklaştır.
- **Sol tık** rengi seçer ve kopyalar. **Sağ tık** veya `Esc` vazgeçer.
- Seçimden sonra açılan editörde renk geçmişi, açık/koyu tonlar ve 9 format var:
  HEX, RGB, HSL, HSV, OKLCH, CMYK, HWB, Decimal, Float.
  Seçimde kopyalanacak format **Ayarlar → Renk seçici**'den belirlenir.

### Komut satırı

```sh
yaltri                    # arka planda başlat
yaltri toggle launcher    # arama kutusunu aç / kapat
yaltri toggle picker      # renk seçiciyi aç
yaltri settings           # ayarları aç
yaltri quit               # kapat
yaltri uninstall          # kaldır (Windows)
```

### Linux'ta kısayollar

- **GNOME (X11 ve Wayland):** Yaltri kısayollarını GNOME'un özel kısayollarına kendisi ekler ve Ayarlar'la
  senkron tutar. GNOME aynı tuşları zaten kullanıyorsa (örneğin `Super+Space` klavye dilini değiştirir),
  Yaltri çalıştığı sürece onları devralır; kısayolu değiştirince veya çıkınca geri verir.
- **Diğer Wayland masaüstleri** uygulamaların global kısayol tanımlamasına izin vermez. Kısayolu masaüstü
  ayarlarından ekle, komut olarak `yaltri toggle launcher` (veya `yaltri toggle picker`) yaz:
  - **KDE:** Sistem Ayarları → Kısayollar → Özel Kısayollar
  - **Hyprland:** `bind = SUPER SHIFT, F, exec, yaltri toggle launcher`

---

## Ayarlar ve dosyalar

|  | Windows | Linux |
|---|---|---|
| Ayarlar | `%APPDATA%\yaltri\config\settings.toml` | `~/.config/yaltri/settings.toml` |
| Index, geçmiş, loglar | `%LOCALAPPDATA%\yaltri\data` | `~/.local/share/yaltri` |

**Windows'ta kaldırmak için:** Ayarlar → Uygulamalar → Yüklü uygulamalar → Yaltri → **Kaldır**. Ayarların ve
geçmişin de silinsin mi diye sorar.

**Linux'ta kaldırmak için:** Ayarlar'da **Sistem açılışında başlat**'ı kapat, tepsi menüsünden **Çıkış**'a bas, sonra
`~/.local/bin/yaltri` dosyasını, yukarıdaki iki klasörü, `~/.local/share/applications/yaltri.desktop` ve
`~/.local/share/icons/hicolor/scalable/apps/yaltri.svg` dosyalarını sil.

---

## Geliştirme

Katkıya açığız: hata bildirimi ve öneriler için [issue](https://github.com/emrebal98/yaltri/issues) açabilirsin.
Ortam kurulumu, sık kullanılan komutlar, proje yapısı, commit kuralları ve sürüm yayınlama [CONTRIBUTING.tr.md](CONTRIBUTING.tr.md) dosyasında.

---

## Lisans

Copyright (C) 2026 Yaltri katkıcıları.

Yaltri özgür bir yazılımdır: [GNU Genel Kamu Lisansı v3.0 veya sonrası](LICENSE) şartlarıyla kullanabilir,
inceleyebilir, paylaşabilir ve değiştirebilirsin. Değiştirilmiş bir sürümü ya da Yaltri'nin kodundan yapılmış
bir programı dağıtırsan, kaynak kodunu da aynı lisansla paylaşmak zorundasın.

İkonlar [Lucide](https://lucide.dev)'dan ([ISC](assets/icons/LICENSE)).
