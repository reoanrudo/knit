package com.google.android.apps.inputmethod.libs.mozc.session;

/** Binding to the BSD-licensed Mozc JNI library's upstream entry point. */
public final class MozcJni {
    private MozcJni() {}
    public static native boolean initialize();
    public static native boolean onPostLoad(String profile, String dictionary);
    public static native byte[] evalCommand(byte[] command);
    public static native String getDataVersion();
}
