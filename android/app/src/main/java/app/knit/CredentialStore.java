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
        Cipher c=Cipher.getInstance("AES/GCM/NoPadding"); c.init(Cipher.DECRYPT_MODE,key(),new GCMParameterSpec(128,Base64.decode(prefs.getString("iv",""),Base64.NO_WRAP)));
        return new JSONObject(new String(c.doFinal(Base64.decode(prefs.getString("sealed",""),Base64.NO_WRAP)),java.nio.charset.StandardCharsets.UTF_8));
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
