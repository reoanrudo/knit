package app.knit;

import android.content.Context;
import android.os.Handler;
import android.os.Looper;
import com.google.android.apps.inputmethod.libs.mozc.session.MozcJni;
import org.mozc.android.inputmethod.japanese.protobuf.ProtoCommands.*;
import org.mozc.android.inputmethod.japanese.protobuf.ProtoConfig.Config;
import java.io.*;
import java.nio.file.Files;
import java.nio.file.StandardCopyOption;
import java.util.concurrent.Executors;
import java.util.concurrent.ExecutorService;

/** Offline Mozc sessions; all calls are serialized, each editor gets a fresh session. */
final class JapaneseEngine implements AutoCloseable {
    static final String REVISION="a069a88d4cb5c011de0f9aebb6c149a1c808d904";
    private static final ExecutorService loader=Executors.newSingleThreadExecutor();
    private static boolean loaded;
    private long session;
    interface Ready { void ready(JapaneseEngine engine, Exception error); }

    static void prepare(Context context,Ready callback) {
        Context app=context.getApplicationContext();
        loader.execute(()->{
            JapaneseEngine engine=null;Exception error=null;
            try {
                synchronized(MozcJni.class) {
                    if(!loaded) {
                        File directory=new File(app.getFilesDir(),"japanese");
                        if(!directory.isDirectory()&&!directory.mkdirs())throw new IOException("日本語辞書の保存先を作成できません。");
                        File dictionary=new File(directory,"mozc-"+REVISION+".data");
                        if(!dictionary.isFile()) {
                            File temp=new File(directory,"dictionary.tmp");
                            try(InputStream in=app.getAssets().open("mozc.data");OutputStream out=new FileOutputStream(temp)) {byte[] buffer=new byte[65536];int size;while((size=in.read(buffer))!=-1)out.write(buffer,0,size);}
                            if(temp.length()<1024*1024)throw new IOException("日本語辞書が不完全です。");
                            Files.move(temp.toPath(),dictionary.toPath(),StandardCopyOption.REPLACE_EXISTING);
                        }
                        System.loadLibrary("mozc");
                        if(!MozcJni.initialize()||!MozcJni.onPostLoad(directory.getAbsolutePath(),dictionary.getAbsolutePath())||MozcJni.getDataVersion().isEmpty())throw new IOException("日本語変換エンジンを開始できません。");
                        loaded=true;
                    }
                    engine=new JapaneseEngine();
                }
            } catch(Exception|LinkageError e) {error=new IOException("日本語入力を準備できません。Knitアプリを更新してください。",e);}
            JapaneseEngine result=engine;Exception failure=error;
            new Handler(Looper.getMainLooper()).post(()->callback.ready(result,failure));
        });
    }
    private JapaneseEngine() throws IOException {
        eval(Input.newBuilder().setType(Input.CommandType.SET_CONFIG).setConfig(Config.newBuilder()
            .setPreeditMethod(Config.PreeditMethod.ROMAN).setSessionKeymap(Config.SessionKeymap.MSIME)
            .setIncognitoMode(true).setHistoryLearningLevel(Config.HistoryLearningLevel.NO_HISTORY)
            .setSuggestionsSize(9).setUseHistorySuggest(false).build()).build());
        Output created=eval(Input.newBuilder().setType(Input.CommandType.CREATE_SESSION).build());
        session=created.getId();
        if(session==0)throw new IOException("日本語入力のセッションを作成できません。");
        eval(Input.newBuilder().setType(Input.CommandType.SET_REQUEST).setId(session)
            .setRequest(Request.newBuilder().setMixedConversion(false).setZeroQuerySuggestion(false).build()).build());
    }
    private static Output eval(Input input) throws IOException {
        synchronized(MozcJni.class) {
            Command out=Command.parseFrom(MozcJni.evalCommand(Command.newBuilder().setInput(input).build().toByteArray()));
            if(!out.hasOutput())throw new IOException("日本語変換から応答がありません。");
            return out.getOutput();
        }
    }
    Output key(int kc,boolean shift,boolean ctrl,boolean alt,CompositionMode mode) throws IOException {
        KeyEvent key=JapaneseKeys.event(kc,shift,ctrl,alt,mode!=CompositionMode.HALF_ASCII);
        if(key==null)return null;
        return eval(Input.newBuilder().setType(Input.CommandType.SEND_KEY).setId(session)
            .setKey(key.toBuilder().setMode(mode).build()).build());
    }
    Output command(SessionCommand command) throws IOException {
        return eval(Input.newBuilder().setType(Input.CommandType.SEND_COMMAND).setId(session).setCommand(command).build());
    }
    Output mode(CompositionMode mode) throws IOException {
        return command(SessionCommand.newBuilder().setType(mode==CompositionMode.HALF_ASCII?SessionCommand.CommandType.TURN_OFF_IME:SessionCommand.CommandType.TURN_ON_IME)
            .setCompositionMode(mode).build());
    }
    void reset() throws IOException { command(SessionCommand.newBuilder().setType(SessionCommand.CommandType.RESET_CONTEXT).build()); }
    @Override public void close() {
        if(session!=0) {
            try {eval(Input.newBuilder().setType(Input.CommandType.DELETE_SESSION).setId(session).build());}catch(IOException ignored){}
            session=0;
        }
    }
}
