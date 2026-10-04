package app.knit;
import org.junit.Test;
import java.util.concurrent.atomic.AtomicBoolean;
import java.util.concurrent.atomic.AtomicReference;
import static org.junit.Assert.*;

/**
 * bulk 確立直後の選択解除(deselect)が重なった時に、取り出し済みの保留共有が
 * 黙って消えるのを防ぐ再キュー状態遷移の検証。汎型メソッド経由で JSON クラスに
 * 依存せず状態だけを確かめる(android.jar のスタブは JVM 上で動かないため)
 */
public class PendingFilesTest {
    @Test public void firstFailureReturnsFilesToPendingOnce() {
        AtomicReference<String> pending=new AtomicReference<>(null);
        AtomicBoolean retried=new AtomicBoolean(false);
        assertTrue(ConnectionService.requeueOnFailure(pending,retried,"/a /b",false));
        assertEquals("/a /b",pending.get());
        assertTrue(retried.get());
        // 戻した分が再度失敗しても、もう戻さない(無限再送の防止)
        assertFalse(ConnectionService.requeueOnFailure(pending,retried,"/a /b",true));
        assertEquals("/a /b",pending.get());
    }

    @Test public void newerShareIsNeverOverwritten() {
        AtomicReference<String> pending=new AtomicReference<>("/new");
        AtomicBoolean retried=new AtomicBoolean(false);
        assertFalse(ConnectionService.requeueOnFailure(pending,retried,"/old",false));
        assertEquals("/new",pending.get());
        assertFalse(retried.get());
    }

    @Test public void emptyPendingAcceptsTheRetryMarker() {
        // 未取り出しの失敗(通常の送信失敗)も 1 までは保留へ戻せる
        AtomicReference<String> pending=new AtomicReference<>(null);
        AtomicBoolean retried=new AtomicBoolean(false);
        assertTrue(ConnectionService.requeueOnFailure(pending,retried,"/x",false));
        assertTrue(retried.get());
        // 同じ失敗の繰り返し扱い(wasRetry)は上の検証の通り戻らない
    }

    /** 起動時の送信キャッシュ掃除: 保留が指す実体の UUID ディレクトリだけを
     *  掃除から守り、ほかの残骸(未送信のまま再起動した 1 件最大 256MiB の
     *  実体)は削除対象にする */
    @Test public void sendCacheCleanKeepsOnlyPendingBodies() {
        String[] pending={"/data/user/0/app.knit/cache/send/uuid-keep/photo.jpg"};
        assertTrue(ConnectionService.isProtectedFromClean("/data/user/0/app.knit/cache/send/uuid-keep",pending));
        assertFalse(ConnectionService.isProtectedFromClean("/data/user/0/app.knit/cache/send/uuid-old",pending));
        // 前方一致で誤保護しない(uuid-keepX は uuid-keep の実体を含まない)
        assertFalse(ConnectionService.isProtectedFromClean("/data/user/0/app.knit/cache/send/uuid-keepX",pending));
        // 保留が空なら全部掃除対象
        assertFalse(ConnectionService.isProtectedFromClean("/data/user/0/app.knit/cache/send/uuid-keep",new String[0]));
    }
}
