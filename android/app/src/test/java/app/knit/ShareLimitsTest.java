package app.knit;
import org.junit.Test;
import static org.junit.Assert.*;

/**
 * 共有受付上限の境界(件数 512・合計 10GiB)。UI と ContentResolver から切り離した
 * 純関数の検証(android.jar のスタブは JVM 上で動かないため、判定だけを確かめる)
 */
public class ShareLimitsTest {
    @Test public void withinLimitsAccepts() {
        assertNull(ShareLimits.reject(0, 0));
        assertNull(ShareLimits.reject(1, 0));
        // 境界値ちょうどは通す(超えた分から拒否)
        assertNull(ShareLimits.reject(ShareLimits.MAX_COUNT, 0));
        assertNull(ShareLimits.reject(1, ShareLimits.MAX_TOTAL_BYTES));
        assertNull(ShareLimits.reject(ShareLimits.MAX_COUNT, ShareLimits.MAX_TOTAL_BYTES));
    }

    @Test public void countOverLimitIsRejected() {
        assertNotNull(ShareLimits.reject(ShareLimits.MAX_COUNT + 1, 0));
        assertNotNull(ShareLimits.reject(Integer.MAX_VALUE, 0));
    }

    @Test public void totalOverLimitIsRejected() {
        assertNotNull(ShareLimits.reject(1, ShareLimits.MAX_TOTAL_BYTES + 1));
        // 1 件 256MiB 上限の実体コピー 41 件(=10.25GiB)は累積上限を超える
        assertNotNull(ShareLimits.reject(41, 41L * 256 * 1024 * 1024));
        assertNotNull(ShareLimits.reject(1, Long.MAX_VALUE));
    }

    @Test public void messageTellsTheLimits() {
        // 案内には上限の数字が含まれる(次にどうすればよいか分かるように)
        assertTrue(ShareLimits.reject(ShareLimits.MAX_COUNT + 1, 0).contains("512"));
        assertTrue(ShareLimits.reject(1, ShareLimits.MAX_TOTAL_BYTES + 1).contains("10GiB"));
    }
}
