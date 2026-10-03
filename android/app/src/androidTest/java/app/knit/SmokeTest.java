package app.knit;
import android.app.*;
import android.os.*;
import android.content.*;
import android.widget.*;
import android.view.*;
import android.view.inputmethod.InputMethodManager;
import org.json.*;
import java.util.concurrent.atomic.AtomicInteger;
/** Runs only in a disposable emulator; no personal device or audio is touched. */
public class SmokeTest extends Instrumentation {
    private Bundle arguments;
    @Override public void onCreate(Bundle args){arguments=args;start();}
    private void check(boolean ok,String label)throws Exception{if(!ok)throw new Exception(label);}
    private void waitFor(java.util.function.BooleanSupplier ready,String label)throws Exception{long until=SystemClock.uptimeMillis()+15000;while(!ready.getAsBoolean()&&SystemClock.uptimeMillis()<until)Thread.sleep(50);check(ready.getAsBoolean(),label);}
    @Override public void onStart(){Bundle result=new Bundle();long socket=0;
        try {
            if("tablet_ime".equals(arguments.getString("mode"))) {tabletIme();result.putString("stream","PASS: offline Mozc, tablet preedit and candidates, Space/Enter, full-width, cancellation, ASCII, editor isolation\n");finish(Activity.RESULT_OK,result);return;}
            Context context=getTargetContext();CredentialStore store=new CredentialStore(context);
            if(!store.paired()) Native.request(1,0,Native.obj("address","10.0.2.2","code",arguments.getString("code")),store);
            JSONObject credentials=store.load();check(credentials!=null,"Keystore enrollment not saved");check(credentials.getString("address").equals("10.0.2.2:34900"),"wrong enrollment port");
            socket=((JSONObject)Native.request(2,0,credentials,null)).getLong("handle");
            Native.request(4,socket,Native.obj("t","hello","ver",13,"name","Unicode test","w",1080,"h",1920,"id","android-app-test"),null);
            check(((JSONObject)Native.request(3,socket,Native.obj(),null)).getString("t").equals("hello_ok"),"hello handshake");
            Native.request(4,socket,Native.obj("t","text","text","日本語🍵"),null);
            JSONObject echo=(JSONObject)Native.request(3,socket,Native.obj(),null);check(echo.getString("text").equals("日本語🍵"),"JNI Unicode text damaged");Native.close(socket);socket=0;
            MainActivity activity=(MainActivity)startActivitySync(new Intent(context,MainActivity.class).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK));
            context.startForegroundService(new Intent(context,ConnectionService.class));waitFor(()->ConnectionService.connected,"foreground service connection");waitFor(()->ControlService.current!=null,"accessibility permission");
            AtomicInteger taps=new AtomicInteger(),moves=new AtomicInteger(),pinch=new AtomicInteger();
            View[] touch=new View[1];
            runOnMainSync(()->{touch[0]=new View(activity);touch[0].setBackgroundColor(0xffedf2ff);touch[0].setOnTouchListener((v,e)->{if(e.getActionMasked()==MotionEvent.ACTION_UP)taps.incrementAndGet();if(e.getActionMasked()==MotionEvent.ACTION_MOVE)moves.incrementAndGet();if(e.getPointerCount()==2)pinch.incrementAndGet();return true;});activity.setContentView(touch[0]);});
            Thread.sleep(400);runOnMainSync(()->{ControlService.current.receive(Native.obj("t","warp","nx",.5,"ny",.5));ControlService.current.receive(Native.obj("t","mouse_btn","btn",0,"down",true));});Thread.sleep(120);runOnMainSync(()->ControlService.current.receive(Native.obj("t","mouse_btn","btn",0,"down",false)));waitFor(()->taps.get()>0,"tap not delivered");
            runOnMainSync(()->ControlService.current.receive(Native.obj("t","mouse_btn","btn",0,"down",true)));Thread.sleep(120);runOnMainSync(()->ControlService.current.receive(Native.obj("t","mouse_abs","nx",.7,"ny",.6)));Thread.sleep(120);runOnMainSync(()->ControlService.current.receive(Native.obj("t","mouse_btn","btn",0,"down",false)));waitFor(()->moves.get()>0,"drag not delivered");
            Thread.sleep(200);runOnMainSync(()->ControlService.current.receive(Native.obj("t","pinch","phase",0,"delta",0)));Thread.sleep(100);runOnMainSync(()->ControlService.current.receive(Native.obj("t","pinch","phase",1,"delta",.5)));Thread.sleep(100);runOnMainSync(()->ControlService.current.receive(Native.obj("t","pinch","phase",2,"delta",0)));waitFor(()->pinch.get()>0,"pinch not delivered");
            EditText[] editor=new EditText[1];runOnMainSync(()->{editor[0]=new EditText(activity);editor[0].setSingleLine(true);activity.setContentView(editor[0]);editor[0].requestFocus();((InputMethodManager)activity.getSystemService(Context.INPUT_METHOD_SERVICE)).showSoftInput(editor[0],InputMethodManager.SHOW_IMPLICIT);});waitFor(()->KnitIme.current!=null&&KnitIme.current.getCurrentInputConnection()!=null,"Knit IME not attached");
            runOnMainSync(()->KnitIme.current.receive(Native.obj("t","text","text","日本語🍵")));waitFor(()->editor[0].getText().toString().equals("日本語🍵"),"Japanese input failed");
            runOnMainSync(()->{KnitIme.current.receive(Native.obj("t","ime","kana",false));KnitIme.current.receive(Native.obj("t","key","kc",0,"down",true,"shift",true));});waitFor(()->editor[0].getText().toString().equals("日本語🍵A"),"keyboard input failed");
            String filename="knit-e2e-"+System.currentTimeMillis()+".txt";
            java.io.File incoming=new java.io.File(context.getFilesDir(),filename);
            try(java.io.FileOutputStream out=new java.io.FileOutputStream(incoming)){out.write("共有データ🍵".getBytes(java.nio.charset.StandardCharsets.UTF_8));}
            Sharing.receiveFiles(context,Native.obj("files",new JSONArray().put(incoming.getAbsolutePath())));
            try(android.database.Cursor cursor=context.getContentResolver().query(android.provider.MediaStore.Downloads.EXTERNAL_CONTENT_URI,new String[]{"_id"},"_display_name=?",new String[]{filename},null)){
                check(cursor!=null&&cursor.moveToFirst(),"MediaStore file publication failed");android.net.Uri saved=android.content.ContentUris.withAppendedId(android.provider.MediaStore.Downloads.EXTERNAL_CONTENT_URI,cursor.getLong(0));
                try(java.io.InputStream in=context.getContentResolver().openInputStream(saved)){byte[] data=new byte[64];int n=in.read(data);check(new String(data,0,n,java.nio.charset.StandardCharsets.UTF_8).equals("共有データ🍵"),"MediaStore bytes damaged");}
                context.getContentResolver().delete(saved,null,null);
            }
            context.stopService(new Intent(context,ConnectionService.class));waitFor(()->!ConnectionService.connected,"stop did not disconnect");
            runOnMainSync(()->activity.recreate());Thread.sleep(300);
            result.putString("stream","PASS: PAKE+Keystore, Noise+JNI Unicode, foreground connection, Accessibility tap/drag/pinch, IME Japanese+key, MediaStore file, stop\n");finish(Activity.RESULT_OK,result);
        }catch(Exception e){result.putString("stream","FAIL: "+e+"\n");finish(Activity.RESULT_CANCELED,result);}
        finally{Native.close(socket);}
    }
    private void key(int kc)throws Exception{runOnMainSync(()->KnitIme.current.receive(Native.obj("t","key","kc",kc,"down",true)));Thread.sleep(30);}
    private void keys(int... keys)throws Exception{for(int kc:keys)key(kc);}
    private Button findButton(View root,String text){if(root instanceof Button&&((Button)root).getText().toString().equals(text))return (Button)root;if(root instanceof ViewGroup)for(int i=0;i<((ViewGroup)root).getChildCount();i++){Button found=findButton(((ViewGroup)root).getChildAt(i),text);if(found!=null)return found;}return null;}
    private void tabletIme()throws Exception{
        Context context=getTargetContext();MainActivity activity=(MainActivity)startActivitySync(new Intent(context,MainActivity.class).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK));
        check(android.os.Build.HARDWARE.equals("ranchu")&&android.os.Build.MODEL.startsWith("sdk_"),"tablet IME instrumentation requires a disposable emulator");
        try(ParcelFileDescriptor descriptor=getUiAutomation().executeShellCommand("ime set app.knit/.KnitIme");java.io.InputStream stream=new ParcelFileDescriptor.AutoCloseInputStream(descriptor)){stream.readAllBytes();}
        EditText[] fields=new EditText[2];
        runOnMainSync(()->{LinearLayout row=new LinearLayout(activity);row.setOrientation(LinearLayout.VERTICAL);for(int i=0;i<2;i++){fields[i]=new EditText(activity);fields[i].setSingleLine(true);row.addView(fields[i]);}activity.setContentView(row);fields[0].requestFocus();((InputMethodManager)activity.getSystemService(Context.INPUT_METHOD_SERVICE)).showSoftInput(fields[0],InputMethodManager.SHOW_IMPLICIT);});
        waitFor(()->KnitIme.japaneseReady()&&KnitIme.current.getCurrentInputConnection()!=null,"offline Japanese engine did not initialize");
        runOnMainSync(()->((InputMethodManager)activity.getSystemService(Context.INPUT_METHOD_SERVICE)).showSoftInput(fields[0],InputMethodManager.SHOW_IMPLICIT));
        waitFor(()->KnitIme.current.isInputViewShown(),"tablet candidate view not visible");
        keys(104,45,34,4,31,45,45);
        waitFor(()->fields[0].getText().toString().equals("にほん"),"preedit is not shown in the tablet editor");
        check(android.view.inputmethod.BaseInputConnection.getComposingSpanStart(fields[0].getText())>=0,"preedit was prematurely committed");
        key(49);waitFor(()->fields[0].getText().toString().equals("日本"),"Space did not convert into kanji");
        waitFor(()->{Button focused=findButton(KnitIme.current.getWindow().getWindow().getDecorView(),"日本");return focused!=null&&focused.getCurrentTextColor()==android.graphics.Color.rgb(47,80,182);},"tablet conversion candidate row missing");
        if(arguments.getBoolean("capture",false)||"true".equals(arguments.getString("capture"))) {
            waitForIdleSync();Thread.sleep(250);
            android.graphics.Bitmap screenshot=getUiAutomation().takeScreenshot();check(screenshot!=null,"candidate screenshot unavailable");
            try(java.io.OutputStream out=new java.io.FileOutputStream(new java.io.File(context.getFilesDir(),"tablet-ime-preview.png"))){screenshot.compress(android.graphics.Bitmap.CompressFormat.PNG,100,out);}finally{screenshot.recycle();}
        }
        Button[] page=new Button[1];runOnMainSync(()->{page[0]=findButton(KnitIme.current.getWindow().getWindow().getDecorView(),"›");if(page[0]!=null)page[0].performClick();});
        check(page[0]!=null&&findButton(KnitIme.current.getWindow().getWindow().getDecorView(),"‹")!=null,"candidate paging failed");
        runOnMainSync(()->findButton(KnitIme.current.getWindow().getWindow().getDecorView(),"‹").performClick());
        key(36);check(fields[0].getText().toString().equals("日本"),"Enter duplicated or inserted newline");
        check(android.view.inputmethod.BaseInputConnection.getComposingSpanStart(fields[0].getText())<0,"Enter did not finish composition");
        runOnMainSync(()->fields[0].setText(""));keys(104,4,0,1,34,49); // hasi + conversion
        Button[] choice=new Button[1];waitFor(()->{choice[0]=findButton(KnitIme.current.getWindow().getWindow().getDecorView(),"箸");return choice[0]!=null;},"alternative candidate missing");
        runOnMainSync(()->choice[0].performClick());waitFor(()->fields[0].getText().toString().equals("箸"),"candidate selection did not commit");
        runOnMainSync(()->fields[0].setText(""));keys(104,0,11,8,18,19,20,101,36);
        waitFor(()->fields[0].getText().toString().equals("ａｂｃ１２３"),"F9 full-width conversion failed");
        runOnMainSync(()->fields[0].setText(""));keys(104,45,34,4,31,45,45,49,53,53);
        waitFor(()->fields[0].getText().toString().isEmpty(),"Escape did not cancel conversion/preedit");
        keys(102,0,11,8);waitFor(()->fields[0].getText().toString().equals("abc"),"Eisu/ASCII input regressed");
        runOnMainSync(()->fields[0].setText(""));keys(104,45,34,4,31,45,45,51,123,32,36);
        waitFor(()->fields[0].getText().toString().equals("にうほ"),"Backspace or composition cursor editing failed");
        runOnMainSync(()->{fields[0].setText("前後");fields[0].setSelection(1);});keys(104,45,34,4,31,45,45,49,36);
        waitFor(()->fields[0].getText().toString().equals("前日本後"),"conversion overwrote surrounding text");
        check(fields[0].getSelectionStart()==3,"committed caret position is wrong");
        runOnMainSync(()->fields[0].setText(""));keys(104,45,34,4,31,45,45);
        runOnMainSync(()->fields[0].setSelection(1));Thread.sleep(120);keys(32,36);
        waitFor(()->fields[0].getText().toString().equals("にうほん"),"tablet cursor movement did not update composition");
        runOnMainSync(()->fields[0].setText(""));key(104);
        runOnMainSync(()->findButton(KnitIme.current.getWindow().getWindow().getDecorView(),"あ").performClick());
        runOnMainSync(()->findButton(KnitIme.current.getWindow().getWindow().getDecorView(),"A").performClick());
        keys(0,11,8,18,19,20,36);waitFor(()->fields[0].getText().toString().equals("ａｂｃ１２３"),"tablet full-width mode failed");
        runOnMainSync(()->fields[0].setText(""));keys(104,45,34);
        waitFor(()->fields[0].getText().toString().equals("に"),"preedit setup failed");
        runOnMainSync(()->{fields[1].requestFocus();((InputMethodManager)activity.getSystemService(Context.INPUT_METHOD_SERVICE)).showSoftInput(fields[1],InputMethodManager.SHOW_IMPLICIT);});Thread.sleep(300);
        keys(32,36);waitFor(()->fields[1].getText().toString().equals("う"),"composition leaked across editors");
        runOnMainSync(()->activity.finish());
    }
}
