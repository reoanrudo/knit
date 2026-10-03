package app.knit;
import java.nio.charset.StandardCharsets;
import org.json.*;
final class Native {
    static { System.loadLibrary("knit_android"); }
    static native byte[] call(int op, long handle, byte[] args, CredentialStore store);
    static Object request(int op, long handle, JSONObject args, CredentialStore store) throws Exception {
        byte[] reply = call(op,handle,args.toString().getBytes(StandardCharsets.UTF_8),store);
        if (reply == null) throw new Exception("通信を処理できませんでした。");
        Object value = new JSONTokener(new String(reply,StandardCharsets.UTF_8)).nextValue();
        if (value instanceof JSONObject && ((JSONObject)value).has("error")) throw new Exception(((JSONObject)value).getString("error"));
        return value;
    }
    static JSONObject obj(Object... pairs) {
        JSONObject o = new JSONObject();
        try { for (int i=0;i<pairs.length;i+=2) o.put((String)pairs[i],pairs[i+1]); }
        catch (JSONException e) { throw new IllegalArgumentException(e); }
        return o;
    }
    static void close(long id) { if(id!=0) try { request(5,id,obj(),null); } catch(Exception ignored) {} }
}
