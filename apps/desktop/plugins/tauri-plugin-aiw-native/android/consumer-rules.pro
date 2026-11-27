# JNI entry point called by the Rust library (`Java_dev_aiworkspace_nativellm_BackgroundBridge_nativeTick`): R8 must keep the
# class and the native method under their names, or the background worker fails with UnsatisfiedLinkError.
-keep class dev.aiworkspace.nativellm.BackgroundBridge { *; }
# The plugin and the types Tauri fills from JSON (@InvokeArg) are looked up by name.
-keep class dev.aiworkspace.nativellm.** { *; }
