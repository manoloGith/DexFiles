# The activity and the provider are found by name from the manifest, and MainActivity.files is
# called over JNI. craft-activity's consumer rules keep its own classes and GameActivity's.
-keep class io.github.dexfiles.** { *; }
