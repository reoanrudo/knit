import android.content.ClipData;
import android.content.ClipDescription;
import android.content.ClipboardManager;
import android.content.Context;
import android.net.Uri;

/**
 * Knit のクリップボード画像設定ヘルパー。scrcpy のサーバー部品と同じ方式で
 * app_process から一時的に動く(apk のインストール不要。CLASSPATH にこの jar を
 * 指定して起動する)。起動引数: <content URI> <MIME>。
 * 画像自体は Mac 側から adb push で /sdcard/Pictures/Knit へ置かれ、MediaStore に
 * 自動登録済みの URI を受け取る。ContentResolver を使うと shell uid での
 * プロバイダ取得に失敗するため、MIME は受け取った値で直接 ClipDescription へ
 * 載せる(プロバイダへはアクセスしない)。成功時は "ok" を標準出力へ出す
 */
public final class ClipSetter {

    public static void main(String[] args) {
        try {
            if (args.length < 2) {
                System.out.println("err: args <uri> <mime>");
                System.exit(2);
            }
            // ClipboardManager の生成に Handler(Looper) が必要
            if (android.os.Looper.myLooper() == null) {
                android.os.Looper.prepare();
            }
            Uri uri = Uri.parse(args[0]);
            String mime = args[1];
            Context ctx = shellContext();
            ClipboardManager cm =
                    (ClipboardManager) ctx.getSystemService(Context.CLIPBOARD_SERVICE);
            // ClipboardManager が内部に保持する Context を差し替える(scrcpy の
            // FakeContext.getSystemService と同じ。binder 呼び出しの呼び出し元検証
            // (AttributionSource)が shell の物になり、SecurityException を避ける)
            java.lang.reflect.Field inner = cm.getClass().getDeclaredField("mContext");
            inner.setAccessible(true);
            inner.set(cm, ctx);
            ClipDescription desc = new ClipDescription("Knit", new String[]{mime});
            cm.setPrimaryClip(new ClipData(desc, new ClipData.Item(uri)));
            System.out.println("ok");
        } catch (Throwable t) {
            Throwable cause = t.getCause();
            System.out.println("err: " + t + (cause != null ? " / cause: " + cause : ""));
            System.exit(1);
        }
    }

    /**
     * システムの Context を、パッケージ名を "com.android.shell" に偽装して返す
     * (scrcpy の FakeContext と同じ。クリップボードサービスの呼び出し元検証は
     * AttributionSource で行われるため、そちらも shell の物を作る)
     */
    private static Context shellContext() throws Exception {
        Context base = systemContext();
        return new android.content.ContextWrapper(base) {
            @Override
            public String getPackageName() {
                return "com.android.shell";
            }

            @Override
            public String getOpPackageName() {
                return "com.android.shell";
            }

            @Override
            public android.content.AttributionSource getAttributionSource() {
                // Process.SHELL_UID = 2000
                android.content.AttributionSource.Builder b =
                        new android.content.AttributionSource.Builder(2000);
                b.setPackageName("com.android.shell");
                return b.build();
            }
        };
    }

    /**
     * app_process 内でシステムの Context を得る。ActivityThread.systemMain() は
     * system_server 専用で例外になるため、scrcpy の Workarounds と同じく
     * private コンストラクタで new して static フィールドへ登録する
     */
    private static Context systemContext() throws Exception {
        Class<?> at = Class.forName("android.app.ActivityThread");
        java.lang.reflect.Constructor<?> ctor = at.getDeclaredConstructor();
        ctor.setAccessible(true);
        Object thread = ctor.newInstance();
        java.lang.reflect.Field cur = at.getDeclaredField("sCurrentActivityThread");
        cur.setAccessible(true);
        cur.set(null, thread);
        java.lang.reflect.Field sys = at.getDeclaredField("mSystemThread");
        sys.setAccessible(true);
        sys.setBoolean(thread, true);
        java.lang.reflect.Method getCtx = at.getDeclaredMethod("getSystemContext");
        getCtx.setAccessible(true);
        return (Context) getCtx.invoke(thread);
    }

    private ClipSetter() {
    }
}
