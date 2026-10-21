# Windows kabul testi

Bu kontrol listesi gerçek Windows Explorer ve kurulmuş NSIS paketi üzerinde
çalıştırılmalıdır. Linux'taki çapraz derleme kontrolü bu davranışları kanıtlamaz.

## Kurulum ve hızlı menü

1. GitHub Actions'ın `grapefruit-windows-installer` artifact'ındaki NSIS
   kurulumunu çalıştırın ve Grapefruit'u açın.
2. Explorer'da bir dosyayı sol tuşla en az birkaç piksel sürükleyin; sürükleme
   devam ederken yalnızca `Shift` tuşuna basın.
3. Çemberin imlecin çevresinde, ekran sınırları içinde ve şeffaf arka planla
   açıldığını doğrulayın.
4. Sol tuşu bırakmadan `Control` tuşuna basın. Menü dosya işlemlerine geçmeli ve
   ilk açıldığı konumda kalmalıdır. Yeniden `Shift` dönüşüm moduna dönmelidir.
5. Dosyayı uygun bir dilime bırakın. Kaynak dosyanın değişmediğini ve yeni
   çıktının aynı klasörde oluştuğunu doğrulayın.

## Dönüşümler

- JPG/PNG/WebP arasında bir görsel dönüşümü yapın.
- Ses içeren bir videoyu WebM, GIF ve MP3'e dönüştürün.
- Bir PDF'i PNG ve TXT'ye dönüştürün. Bu üç grup, kurulumla gelen FFmpeg ve
  Poppler'ı kullanmalı; ayrıca PATH ayarı gerektirmemelidir.
- LibreOffice kuruluysa DOCX → PDF ve PPTX → PDF/PNG deneyin. LibreOffice kurulu
  değilse belge ve sunum dönüşümünün anlaşılır bir araç-bulunamadı hatası vermesi
  beklenir.
- Aynı dönüşümü iki kez yapın; ikinci çıktı var olan dosyanın üzerine yazmamalı,
  `(1)` ekli yeni ad kullanmalıdır.

## Dosya işlemleri

- Yolu kopyala, üst klasörde göster, kopyala ve taşı işlemlerini ayrı ayrı
  deneyin.
- Birden fazla dosyayı birlikte sürükleyip ortak dönüşüm hedeflerini ve toplu
  dosya işlemlerini kontrol edin.

## Bilinen doğrulama noktası

Windows izleyicisi Explorer'a özel bir hook kullanmaz. Sol tuşla sürükleme ve
Shift/Control birleşimi başka uygulamalarda da menüyü açabilir. Bu davranış kabul
edilemez ölçüde sık yanlış tetikleniyorsa sonraki adım Explorer sürükleme
bağlamını ayırt eden Windows'a özgü bir entegrasyondur.
