# Add project specific ProGuard rules here.
# You can control the set of applied configuration files using the
# proguardFiles setting in build.gradle.
#
# For more details, see
#   http://developer.android.com/guide/developing/tools/proguard.html

# If your project uses WebView with JS, uncomment the following
# and specify the fully qualified class name to the JavaScript interface
# class:
#-keepclassmembers class fqcn.of.javascript.interface.for.webview {
#   public *;
#}

# Uncomment this to preserve the line number information for
# debugging stack traces.
#-keepattributes SourceFile,LineNumberTable

# If you keep the line number information, uncomment this to
# hide the original source file name.
#-renamesourcefileattribute SourceFile

# btleplug's Java companion (vendored under src-tauri/android/droidplug) is
# reached only through JNI, so R8 sees no caller and would strip the lot. The
# release build type sets isMinifyEnabled = true, which makes these mandatory
# rather than precautionary -- without them BLE fails only in release builds.
-keep class com.nonpolynomial.** { *; }
-keep class io.github.gedgygedgy.** { *; }

# Same reasoning for our own JNI entry point: nothing in Kotlin calls it.
-keep class com.cmiami.brickellstatus.NativeBridge { *; }
