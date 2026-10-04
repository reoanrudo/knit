package app.knit;
import android.content.*;
import android.security.keystore.*;
import android.util.Base64;
import java.security.*;
import javax.crypto.*;
import javax.crypto.spec.GCMParameterSpec;
import org.json.*;
final class CredentialStore {
    private final SharedPreferences prefs;
    private static final String ALIAS="knit.connection.v1";
    CredentialStore(Context context) { prefs=context.getSharedPreferences("connection",Context.MODE_PRIVATE); }
    private SecretKey key() throws Exception {
        KeyStore ks=KeyStore.getInstance("AndroidKeyStore"); ks.load(null);
        if(ks.containsAlias(ALIAS)) return (SecretKey)ks.getKey(ALIAS,null);
        KeyGenerator g=KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES,"AndroidKeyStore");
        g.init(new KeyGenParameterSpec.Builder(ALIAS,KeyProperties.PURPOSE_ENCRYPT|KeyProperties.PURPOSE_DECRYPT).setBlockModes(KeyProperties.BLOCK_MODE_GCM).setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE).build());
        return g.generateKey();
    }
    // JNI invokes this synchronously: pairing is acknowledged only after commit.
    public synchronized boolean save(String token,String address) {
        try {
            Cipher c=Cipher.getInstance("AES/GCM/NoPadding"); c.init(Cipher.ENCRYPT_MODE,key());
            byte[] ciphertext=c.doFinal(Native.obj("token",token,"address",address).toString().getBytes(java.nio.charset.StandardCharsets.UTF_8));
            return prefs.edit().putString("iv",Base64.encodeToString(c.getIV(),Base64.NO_WRAP)).putString("sealed",Base64.encodeToString(ciphertext,Base64.NO_WRAP)).commit();
        } catch(Exception ignored) { return false; }
    }
    synchronized JSONObject load() throws Exception {
        if(!prefs.contains("sealed")) return null;
        try {
            Cipher c=Cipher.getInstance("AES/GCM/NoPadding"); c.init(Cipher.DECRYPT_MODE,key(),new GCMParameterSpec(128,Base64.decode(prefs.getString("iv",""),Base64.NO_WRAP)));
            return new JSONObject(new String(c.doFinal(Base64.decode(prefs.getString("sealed",""),Base64.NO_WRAP)),java.nio.charset.StandardCharsets.UTF_8));
        } catch(Exception e) {
            if(!keyLost(e)) throw e;
            // Keystore の鍵を失った(端末初期化・鍵の無効化・OS更新など)。復号は
            // 二度と成功しないため、壊れた登録をここで自壊して「登録が必要」へ
            // 遷移させる。残したままでは paired() が true を返し続け、
            // 「登録済み」の表示のまま無限再接続ループになる
            prefs.edit().remove("iv").remove("sealed").commit();
            return null;
        }
    }
    /** 復号の失敗が「Keystore の鍵を失って二度と戻らない」種類か。
     *  AEADBadTagException(タグ不一致=鍵が変わって復号できない)と
     *  InvalidKeyException(鍵の無効化。KeyPermanentlyInvalidatedException は
     *  そのサブクラス)を対象とする。SharedPreferences に依存しない純粋な
     *  判定として切り出し、単体テストで検証する */
    static boolean keyLost(Throwable e) {
        return e instanceof AEADBadTagException || e instanceof InvalidKeyException;
    }
    boolean paired() { return prefs.contains("sealed"); }
    void forget() throws Exception {
        if(!prefs.edit().clear().commit()) throw new Exception("登録を削除できませんでした。");
        KeyStore ks=KeyStore.getInstance("AndroidKeyStore"); ks.load(null); ks.deleteEntry(ALIAS);
    }
    String deviceId() {
        String id=prefs.getString("deviceId",null);
        if(id==null) { id="android-app-"+java.util.UUID.randomUUID(); if(!prefs.edit().putString("deviceId",id).commit()) throw new IllegalStateException("端末IDを保存できません。"); }
        return id;
    }
}
