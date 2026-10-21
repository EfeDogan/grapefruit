# Sistem genelinde hızlı menü

## İlk dikey dilim

Uygulama iki pencere kullanır:

- `main`: ayarlar, durum ve normal sürükle-bırak akışı.
- `overlay`: başlangıçta gizli, çerçevesiz, görev çubuğunda görünmeyen ve en üstte
  tutulan 420×420 şeffaf hızlı menü. Bu pencere yalnızca mod seçiciyi, seçili
  dosya önizlemesini ve dairesel hedefleri çizer.

Global kısayol overlay'i seçilen modda açar. Devam eden işletim sistemi
sürükle-bırak işlemi overlay penceresine girdiğinde Tauri dosya yollarını mevcut
dönüşüm ve File Actions komutlarına aktarır.

Varsayılan sistem kısayolları:

- `Ctrl+Shift+F`: Convert formats fallback
- `Ctrl+Shift+A`: File actions

Windows'ta tek başına Shift ve Control normal global kısayol olarak kaydedilmez.
Uygulama sol fare düğmesi basılıyken en az 8 piksel hareketi bir sürükleme adayı
olarak kabul eder ve yalnızca bu sırada tuşların durumunu okur. Tuş olayları
yutulmaz. Diğer platformlarda modifier tuşlarını gözlemek için platforma özgü
entegrasyon kullanılır; `Ctrl+Shift+F/A` güvenli yedek olarak korunur.

## Platform yolları

### Windows

Global fare/klavye durumu güvenli Rust API'si üzerinden kısa aralıklarla okunur.
Sol düğmeyle sürükleme başladıktan sonra Shift/Ctrl kenarı menüyü açar veya açık
menünün modunu değiştirir. Overlay, açılış anındaki global imleç konumundan ve
aktif monitör sınırlarından hesaplanan güvenli bir noktaya yerleştirilir; mod
değişiminde konumu korunur. Tauri global-shortcut eklentisi
`Ctrl+Shift+F/A` yedeğini sağlar.

Windows paketleme işi sabit URL ve SHA-256 değerleriyle FFmpeg ile Poppler'ı
indirir ve Tauri resource olarak NSIS kurulumuna ekler. Çekirdek önce ortam
değişkenlerini ve `PATH`'i, sonra kurulum yanındaki `tools/` dizinini arar.
LibreOffice lisans/boyut nedeniyle paketlenmez; belge ve sunum dönüşümü için
ayrı kurulmalıdır.

### Linux/X11

Windows ile aynı native kısayol motoru ve imleç konumlandırma yolu kullanılır.

### Linux/Wayland

Tauri global-shortcut eklentisinin Wayland desteği güvenilir olmadığı için
`org.freedesktop.portal.GlobalShortcuts` doğrudan `ashpd` üzerinden kullanılır.
Bu portal kısayolları uygulama odağı dışında güvenli biçimde sunar ve masaüstü
ortamının kullanıcı izin arayüzünü korur.

Wayland güvenlik modeli uygulamaların global imleç koordinatını okumasına,
başka bir uygulamadaki seçimi sorgulamasına veya tek başına modifier tuşlarını
genel olarak dinlemesine izin vermez. GNOME dikey dilimi bu yüzden iki dar
entegrasyon kullanır:

- Nautilus `MenuProvider`, geçerli yerel dosya seçimini uygulamanın kullanıcı
  oturumu D-Bus arayüzüne yollar.
- GNOME Shell uzantısı birincil fare düğmesi basılıyken Nautilus veya GNOME
  masaüstünden başlayan sürüklemelerde `Shift`/`Control` tuşlarını gözler.
  İlk `Shift` menüyü açar; sonraki `Shift`/`Control` olayları açık menünün modunu
  değiştirir. İlk açılışta imleç koordinatını alır ve overlay penceresini o
  konumun çevresine taşır. Olayları tüketmez.

Ana uygulama yolları yeniden doğrular, Nautilus seçimini overlay'e olay olarak
aktarır ve menüyü açar. GNOME masaüstünde menü seçim olmadan açılır; gerçek DND
olayı overlay'e girdiğinde dosya yolları yüklenir. Daire dilimleri önceden alınan
veya DND ile gelen seçimle tıklanabilir. GNOME/Wayland üzerinde konumu normal
uygulama değil, yetkili Shell uzantısı compositor API'si üzerinden belirler;
pencere monitörün çalışma alanı dışına taşmayacak şekilde sınırlanır.

## Doğrulama sınırı

Portal arayüzünün bu geliştirme makinesinde bulunduğu ve sürüm 1 sunduğu D-Bus
üzerinden doğrulandı. Kurulu `.deb` kaydı ile host uygulama kimliği başarıyla
kaydedildi; `Ctrl+Shift+A` kullanıcı tarafından gerçek masaüstünde doğrulandı.
GNOME entegrasyonunun D-Bus seçim ve iki menü açma yolları doğrudan çağrılarla
doğrulandı. Shell/Nautilus kullanıcı eklentilerinin gerçek sürükleme testi henüz
kurulum ve etkinleştirme adımını bekliyor.

Windows sürükleme durum makinesi birim testleriyle doğrulandı. Ancak bu Linux
geliştirme makinesinde Explorer, gerçek Windows pencere davranışı, DND teslimi ve
üretilen NSIS kurulumunun uçtan uca çalışması doğrulanamaz; bunlar Windows
makinesindeki kabul testinde kontrol edilmelidir. İzleyici Explorer'a özel bir
hook kullanmadığı için herhangi bir uygulamadaki sol tuş sürüklemesi sırasında
Shift/Ctrl ile de açılabilir.

Kaynaklar:

- [Tauri Global Shortcut](https://v2.tauri.app/plugin/global-shortcut/)
- [Tauri Window API](https://v2.tauri.app/reference/javascript/api/namespacewindow/)
- [XDG GlobalShortcuts portalı](https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.GlobalShortcuts.html)
- [XDG kısayol biçimi](https://specifications.freedesktop.org/shortcuts/latest/)
- [Nautilus MenuProvider](https://gnome.pages.gitlab.gnome.org/nautilus/iface.MenuProvider.html)
