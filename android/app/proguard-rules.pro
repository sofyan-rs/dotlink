-dontwarn org.conscrypt.**
-dontwarn org.bouncycastle.**
-dontwarn org.openjsse.**
-keepclassmembers class com.dotlink.ui.shell.TermBridge { @android.webkit.JavascriptInterface <methods>; }
# ML Kit builds its barcode client from components it finds by reflection; R8 strips their
# fields and BarcodeScanning.getClient() throws a NullPointerException in release builds.
-keep class com.google.mlkit.vision.barcode.internal.** { *; }
-keep class com.google.mlkit.vision.common.internal.** { *; }
-keep class com.google.mlkit.common.internal.** { *; }
