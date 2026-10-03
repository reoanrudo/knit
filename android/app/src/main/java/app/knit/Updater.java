package app.knit;
import android.app.PendingIntent;
import android.content.*;
import android.content.pm.*;
import android.net.Uri;
import android.os.Build;
import android.provider.Settings;
import java.io.*;
import java.net.*;
import java.nio.charset.StandardCharsets;
import java.util.*;
import java.util.concurrent.*;
import org.json.JSONObject;

/**
 * アプリ自体の更新。署名付き更新情報の検証とファイルの検査は Rust 側(Native op 9/10)、
 * 取得と OS へのインストール依頼はここで行う。設計は docs/update-design.md。
 * Android は更新ファイルの署名者が現在のアプリと同じでないとインストールを拒否し、
 * 最後に利用者の確認が入る(自動では入れ替わらない)。
 */
final class Updater {
    enum Phase { IDLE, CHECKING, AVAILABLE, DOWNLOADING, INSTALLING }
    // crates/common/src/update.rs の ANDROID_MANIFEST_URL と同じ値
    static final String MANIFEST_URL = "https://github.com/reoanrudo/knit/releases/download/android-latest/update-android.json";
    static final String RESULT_ACTION = "app.knit.UPDATE_RESULT";
    private static final long MAX_MANIFEST = 64 * 1024;
    private static final long AUTO_INTERVAL_MS = 6L * 3600 * 1000;
    private static final ExecutorService worker = Executors.newSingleThreadExecutor();
    static volatile Phase phase = Phase.IDLE;
    static volatile String message = "";
    private static volatile String version, url, sha256;
    private static volatile long size;
    private static volatile long installingSince;
    private static final long INSTALL_TIMEOUT_MS = 120L * 1000;

    static String current(Context c) {
        try { return c.getPackageManager().getPackageInfo(c.getPackageName(), 0).versionName; }
        catch (PackageManager.NameNotFoundException e) { return "0.0.0"; }
    }

    static String availableVersion() { return version; }

    private static byte[] fetch(String address, long max, File dest) throws IOException {
        URL u = new URL(address);
        if (!"https".equals(u.getProtocol())) throw new IOException("安全でない更新元のため中止しました");
        HttpURLConnection c = (HttpURLConnection) u.openConnection();
        c.setConnectTimeout(15000);
        c.setReadTimeout(30000);
        c.setRequestProperty("User-Agent", "Knit-Android");
        if (c.getResponseCode() != 200) throw new IOException("更新サーバーに接続できません");
        if (c.getContentLengthLong() > max) throw new IOException("ダウンロードした内容が更新情報と一致しません。更新は行いません");
        ByteArrayOutputStream mem = dest == null ? new ByteArrayOutputStream() : null;
        long total = 0;
        try (InputStream in = c.getInputStream(); OutputStream out = dest == null ? mem : new FileOutputStream(dest)) {
            byte[] buf = new byte[64 * 1024];
            int n;
            while ((n = in.read(buf)) > 0) {
                total += n;
                if (total > max) throw new IOException("ダウンロードした内容が更新情報と一致しません。更新は行いません");
                out.write(buf, 0, n);
            }
        } finally { c.disconnect(); }
        return mem == null ? null : mem.toByteArray();
    }

    private static String failure(Exception e) {
        String m = e.getMessage();
        return m == null || m.isEmpty() ? "更新できませんでした" : m;
    }

    /** 更新の確認。silent の時は失敗を利用者に見せない(自動確認用) */
    static void check(Context ctx, boolean silent) {
        if (phase != Phase.IDLE) return;
        Context app = ctx.getApplicationContext();
        phase = Phase.CHECKING;
        if (!silent) message = "";
        worker.execute(() -> {
            try {
                byte[] manifest = fetch(MANIFEST_URL, MAX_MANIFEST, null);
                byte[] sig = fetch(MANIFEST_URL + ".sig", 1024, null);
                JSONObject r = (JSONObject) Native.request(9, 0, Native.obj(
                        "current", current(app),
                        "manifest", new String(manifest, StandardCharsets.UTF_8),
                        "signature", new String(sig, StandardCharsets.UTF_8).trim()), null);
                if (r.optBoolean("latest")) {
                    phase = Phase.IDLE;
                    message = silent ? "" : "最新の版です";
                } else {
                    version = r.getString("version");
                    url = r.getString("url");
                    sha256 = r.getString("sha256");
                    size = r.getLong("size");
                    phase = Phase.AVAILABLE;
                    message = "Knit " + version + " が利用できます";
                }
            } catch (Exception e) {
                phase = Phase.IDLE;
                message = silent ? "" : failure(e);
            }
        });
    }

    /** 起動・再開時に、前回から一定時間が空いていれば静かに確認する */
    static void autoCheck(Context ctx) {
        SharedPreferences p = ctx.getSharedPreferences("update", Context.MODE_PRIVATE);
        long now = System.currentTimeMillis();
        if (phase != Phase.IDLE || now - p.getLong("last_check", 0) < AUTO_INTERVAL_MS) return;
        p.edit().putLong("last_check", now).apply();
        check(ctx, true);
    }

    /** 取得・検査して、OS のインストーラへ渡す(最後の確認は OS の画面で利用者が行う) */
    static void install(Context ctx) {
        if (phase != Phase.AVAILABLE) return;
        Context app = ctx.getApplicationContext();
        PackageManager pm = app.getPackageManager();
        if (!pm.canRequestPackageInstalls()) {
            message = "「この提供元のアプリを許可」を有効にしてから、もう一度更新してください";
            app.startActivity(new Intent(Settings.ACTION_MANAGE_UNKNOWN_APP_SOURCES, Uri.parse("package:" + app.getPackageName())).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK));
            return;
        }
        phase = Phase.DOWNLOADING;
        message = "ダウンロードしています…";
        worker.execute(() -> {
            try {
                File dir = new File(app.getCacheDir(), "updates");
                if (dir.isDirectory()) for (File old : dir.listFiles()) old.delete();
                dir.mkdirs();
                File apk = new File(dir, "Knit-" + version + ".apk");
                fetch(url, size, apk);
                Native.request(10, 0, Native.obj("path", apk.getAbsolutePath(), "size", size, "sha256", sha256), null);
                checkPackage(app, apk);
                // 結果の通知(別スレッド)が先に届いても上書きされないよう、依頼の前に状態を進める
                installingSince = System.currentTimeMillis();
                message = "確認画面で「更新」を選んでください";
                phase = Phase.INSTALLING;
                session(app, apk);
            } catch (Exception e) {
                phase = Phase.IDLE;
                message = failure(e);
            }
        });
    }

    /**
     * 確認画面が出なかった(アプリが裏にいて OS に表示を拒否された等)場合に、ボタンを押せる状態へ戻す。
     * 画面の更新から呼ぶ。
     */
    static void expireInstallIfStuck() {
        if (phase == Phase.INSTALLING && System.currentTimeMillis() - installingSince > INSTALL_TIMEOUT_MS) {
            phase = Phase.AVAILABLE;
            message = "確認画面が表示されませんでした。もう一度「更新」を押してください";
        }
    }

    /** 取り違え・署名者の違いを、OS に拒否される前に分かりやすく伝える */
    private static void checkPackage(Context app, File apk) throws Exception {
        PackageManager pm = app.getPackageManager();
        PackageInfo n = pm.getPackageArchiveInfo(apk.getAbsolutePath(), PackageManager.GET_SIGNING_CERTIFICATES);
        if (n == null || !app.getPackageName().equals(n.packageName)) throw new IOException("更新ファイルが Knit ではありません");
        if (!version.equals(n.versionName)) throw new IOException("更新ファイルの版が更新情報と一致しません");
        PackageInfo cur = pm.getPackageInfo(app.getPackageName(), PackageManager.GET_SIGNING_CERTIFICATES);
        if (n.signingInfo == null || cur.signingInfo == null) throw new IOException("更新ファイルの署名を確認できません");
        Set<String> a = new HashSet<>(), b = new HashSet<>();
        for (Signature s : n.signingInfo.getApkContentsSigners()) a.add(Arrays.toString(s.toByteArray()));
        for (Signature s : cur.signingInfo.getApkContentsSigners()) b.add(Arrays.toString(s.toByteArray()));
        if (a.isEmpty() || !a.equals(b)) throw new IOException("更新ファイルの署名者が現在と異なるため中止しました(開発版では署名が変わることがあります。アンインストール後に再導入してください)");
    }

    private static void session(Context app, File apk) throws IOException {
        PackageInstaller pi = app.getPackageManager().getPackageInstaller();
        PackageInstaller.SessionParams params = new PackageInstaller.SessionParams(PackageInstaller.SessionParams.MODE_FULL_INSTALL);
        params.setAppPackageName(app.getPackageName());
        params.setSize(apk.length());
        int id = pi.createSession(params);
        try (PackageInstaller.Session s = pi.openSession(id)) {
            try (OutputStream out = s.openWrite("Knit.apk", 0, apk.length()); InputStream in = new FileInputStream(apk)) {
                byte[] buf = new byte[64 * 1024];
                int n;
                while ((n = in.read(buf)) > 0) out.write(buf, 0, n);
                s.fsync(out);
            }
            // インストーラが結果の情報を書き込むため、変更可能な PendingIntent にする
            int flags = PendingIntent.FLAG_UPDATE_CURRENT | (Build.VERSION.SDK_INT >= 31 ? PendingIntent.FLAG_MUTABLE : 0);
            Intent result = new Intent(app, UpdateReceiver.class).setAction(RESULT_ACTION);
            s.commit(PendingIntent.getBroadcast(app, id, result, flags).getIntentSender());
        } catch (IOException | RuntimeException e) {
            pi.abandonSession(id);
            throw e;
        }
    }

    /** インストーラからの結果通知を受ける。確認画面の表示と、失敗の理由の反映 */
    static final class UpdateReceiver extends BroadcastReceiver {
        @Override public void onReceive(Context ctx, Intent intent) {
            int status = intent.getIntExtra(PackageInstaller.EXTRA_STATUS, PackageInstaller.STATUS_FAILURE);
            if (status == PackageInstaller.STATUS_PENDING_USER_ACTION) {
                Intent confirm = Build.VERSION.SDK_INT >= 33
                        ? intent.getParcelableExtra(Intent.EXTRA_INTENT, Intent.class)
                        : intent.getParcelableExtra(Intent.EXTRA_INTENT);
                if (confirm != null) ctx.startActivity(confirm.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK));
            } else if (status == PackageInstaller.STATUS_SUCCESS) {
                message = "更新しました。Knit を開き直してください";
                phase = Phase.IDLE;
            } else {
                phase = Phase.IDLE;
                message = status == PackageInstaller.STATUS_FAILURE_ABORTED ? "更新を中止しました" : "更新をインストールできませんでした";
            }
        }
    }
}
