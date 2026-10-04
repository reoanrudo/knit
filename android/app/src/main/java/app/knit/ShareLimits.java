package app.knit;

import android.content.Context;
import android.net.Uri;
import java.util.List;

/**
 * 共有(Intent)で一度に受け付ける上限の判定。本体のドラッグ送信と同じ上限
 * (512 件・合計 10GiB)を共有経路にも課し、際限のない件数・累積コピーを防ぐ。
 * 判定は UI から切り離した純関数にして、JVM 上の単体テストで境界を守る。
 */
final class ShareLimits {
    static final int MAX_COUNT = 512;
    static final long MAX_TOTAL_BYTES = 10L * 1024 * 1024 * 1024;

    private ShareLimits() {}

    /** 上限を超えている時の通知文。収まるときは null(そのまま受け取る) */
    static String reject(int count, long totalBytes) {
        if (count > MAX_COUNT)
            return "共有の件数が多すぎます。一度に送れるのは512件までです。";
        if (totalBytes > MAX_TOTAL_BYTES)
            return "共有の合計サイズが大きすぎます。一度に送れるのは合計10GiBまでです。";
        return null;
    }

    /**
     * ContentResolver で各項目のサイズを数える。サイズの取れない項目は 0 として
     * 数える(スキップして件数をごまかしたり、除外して合計を小さくはしない)。
     */
    static long totalBytes(Context c, List<Uri> uris) {
        long total = 0;
        for (Uri u : uris) {
            try (android.content.res.AssetFileDescriptor a =
                    c.getContentResolver().openAssetFileDescriptor(u, "r")) {
                long len = a != null ? a.getLength() : 0;
                if (len > 0) total += len;
            } catch (Exception ignored) {
            }
        }
        return total;
    }
}
