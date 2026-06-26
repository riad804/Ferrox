# JNA + UniFFI bindings rely on reflection / native names — keep them.
-keep class com.sun.jna.** { *; }
-keep class * implements com.sun.jna.** { *; }
-keep class io.ferrox.sdk.** { *; }
-dontwarn java.awt.**
