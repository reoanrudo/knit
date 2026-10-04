package app.knit;
import org.junit.Test;
import javax.crypto.AEADBadTagException;
import java.security.GeneralSecurityException;
import java.security.InvalidKeyException;
import static org.junit.Assert.*;

/**
 * Keystore 鍵喪失の判定: 復号できない登録を自壊して「登録が必要」へ遷移させる
 * ための分類だけを検証する(SharedPreferences・AndroidKeyStore は JVM 上で
 * 動かないため、判定は純粋な static として切り出してある)
 */
public class CredentialStoreTest {
    @Test public void keyLossExceptionsAreDetected() {
        // タグ不一致: 保存時と別の鍵で復号した(鍵が作り直された)
        assertTrue(CredentialStore.keyLost(new AEADBadTagException("tag mismatch")));
        // 鍵そのものが使えない
        assertTrue(CredentialStore.keyLost(new InvalidKeyException("key gone")));
    }
    /** KeyPermanentlyInvalidatedException は InvalidKeyException のサブクラス。
     *  android.jar のスタブは JVM 上で例外を作れないため、同じ階層の自作例外で
     *  サブクラス経由の検出を確かめる */
    @Test public void invalidatedSubclassIsDetectedViaSuperclass() {
        assertTrue(CredentialStore.keyLost(new LostKey()));
    }
    /** 鍵の喪失以外(保存データの JSON 破損・一時的な失敗)で自壊しない:
     *  一時的な失敗まで登録を消すと、復帰できる登録まで失わせる */
    @Test public void unrelatedFailuresAreNotKeyLoss() {
        assertFalse(CredentialStore.keyLost(new GeneralSecurityException("other")));
        assertFalse(CredentialStore.keyLost(new NullPointerException()));
    }
    private static final class LostKey extends InvalidKeyException {}
}
