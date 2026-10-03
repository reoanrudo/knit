package app.knit;
import android.app.*;
import android.content.*;
import android.os.*;
import android.graphics.Point;
import android.util.DisplayMetrics;
import android.view.WindowManager;
import android.net.Uri;
import android.net.wifi.WifiManager;
import org.json.*;
import java.io.File;
import java.util.concurrent.*;

public final class ConnectionService extends Service {
    static volatile ConnectionService current;
    static volatile String state="未接続";
    static volatile String detail="同じネットワークのMacを登録してください。";
    static volatile boolean connected;
    static volatile boolean clip=true;
    /** 未接続・未選択の間に共有されたファイル(最新1回分だけ保持)。Macとの
     * ファイル経路が通った時に自動で送る。AtomicReference なのは、保持と
     * 取り出し(bulk確立時のgetAndSet)が別スレッドで競合するため */
    static final java.util.concurrent.atomic.AtomicReference<JSONArray> pendingFiles=new java.util.concurrent.atomic.AtomicReference<>();
    private volatile boolean running;
    private volatile long epoch;
    private volatile long handle,bulkHandle;
    private volatile boolean selected;
    private boolean lastControl,lastKeyboard,lastJapanese;
    private final Runnable permissionTicker=new Runnable(){public void run(){
        if(running && connected) {
            boolean control=ControlService.current!=null&&unlocked(),keyboard=KnitIme.selected(ConnectionService.this),japanese=KnitIme.japaneseReady();
            if(control!=lastControl||keyboard!=lastKeyboard||japanese!=lastJapanese) {
                if(!control && ControlService.current!=null) ControlService.current.release();
                permissions();
            }
        }
        if(running) ui.postDelayed(this,1000);
    }};
    private final Handler ui=new Handler(Looper.getMainLooper());
    private WifiManager.WifiLock wifi;
    // Without this, Wi-Fi power saving parks packets for tens of milliseconds and the pointer stutters.
    private void lowLatency(boolean on) {
        try {
            if(on&&wifi==null) { wifi=((WifiManager)getApplicationContext().getSystemService(WIFI_SERVICE)).createWifiLock(WifiManager.WIFI_MODE_FULL_LOW_LATENCY,"Knit input"); wifi.setReferenceCounted(false); }
            if(wifi==null) return;
            if(on&&!wifi.isHeld()) wifi.acquire(); else if(!on&&wifi.isHeld()) wifi.release();
        } catch(RuntimeException ignored) {}
    }
    private final ExecutorService sender=Executors.newSingleThreadExecutor();
    private final ExecutorService files=Executors.newSingleThreadExecutor();
    private static final String CHANNEL="knit.connection";
    // A stop the user asked for is remembered; anything else (reinstall, reboot, killed process) reconnects by itself.
    static boolean stopped(Context c) { return c.getSharedPreferences("knit_run",MODE_PRIVATE).getBoolean("stopped",false); }
    static void setStopped(Context c,boolean v) { c.getSharedPreferences("knit_run",MODE_PRIVATE).edit().putBoolean("stopped",v).apply(); }
    static void autoStart(Context c) {
        if(current!=null||stopped(c)||!new CredentialStore(c).paired()) return;
        try { c.startForegroundService(new Intent(c,ConnectionService.class)); } catch(RuntimeException ignored) {}
    }
    static void report(String message) {
        if(message.equals(detail))return;detail=message;
        ConnectionService service=current;
        if(service!=null)service.ui.post(()->{if(service.running)((NotificationManager)service.getSystemService(NOTIFICATION_SERVICE)).notify(1,service.notification());});
    }
    @Override public void onCreate() { super.onCreate(); current=this; ((NotificationManager)getSystemService(NOTIFICATION_SERVICE)).createNotificationChannel(new NotificationChannel(CHANNEL,"Macとの接続",NotificationManager.IMPORTANCE_LOW)); }
    @Override public int onStartCommand(Intent intent,int flags,int startId) {
        if(intent!=null && "stop".equals(intent.getAction())) { setStopped(this,true); stopSelf(); return START_NOT_STICKY; }
        startForeground(1,notification());
        if(!running) { running=true; ui.post(permissionTicker); long generation=++epoch; new Thread(()->connectLoop(generation),"Knit encrypted connection").start(); }
        return START_NOT_STICKY;
    }
    private Notification notification() {
        Intent stop=new Intent(this,ConnectionService.class).setAction("stop");
        PendingIntent quit=PendingIntent.getService(this,2,stop,PendingIntent.FLAG_IMMUTABLE|PendingIntent.FLAG_UPDATE_CURRENT);
        PendingIntent open=PendingIntent.getActivity(this,1,new Intent(this,MainActivity.class),PendingIntent.FLAG_IMMUTABLE|PendingIntent.FLAG_UPDATE_CURRENT);
        return new Notification.Builder(this,CHANNEL).setSmallIcon(android.R.drawable.ic_menu_share).setContentTitle("Knit · "+state).setContentText(detail).setOngoing(true).setContentIntent(open).addAction(new Notification.Action.Builder(null,"接続を停止",quit).build()).build();
    }
    private void status(String s,String d) {
        // 同一内容の再接続待ちで通知を張り替え続けない(再試行は続いても通知は静かに)
        if(s.equals(state)&&d.equals(detail))return;
        state=s; detail=d; ui.post(()-> { if(running) ((NotificationManager)getSystemService(NOTIFICATION_SERVICE)).notify(1,notification()); }); }
    boolean unlocked() { return !((KeyguardManager)getSystemService(KEYGUARD_SERVICE)).isKeyguardLocked(); }
    private boolean valid(long generation) { return running && epoch==generation; }
    private void connectLoop(long generation) {
        long retry=1000;
        while(valid(generation)) {
            long id=0;
            try {
                JSONObject credentials=new CredentialStore(this).load();
                if(credentials==null) { status("登録が必要","アプリでMacの6桁コードを入力してください。"); break; }
                status("接続中","登録したMacへ暗号化接続しています。");
                id=((JSONObject)Native.request(2,0,credentials,null)).getLong("handle");
                if(!valid(generation)) { Native.close(id); break; }
                handle=id; selected=false;
                Point size=screen();
                sendDirect(id,Native.obj("t","hello","ver",13,"name",Build.MODEL+" · Knitアプリ","w",size.x,"h",size.y,"id",new CredentialStore(this).deviceId(),"monitors",new JSONArray().put(Native.obj("w",size.x,"h",size.y))));
                JSONObject hello=(JSONObject)Native.request(3,id,Native.obj(),null);
                if(!"hello_ok".equals(hello.optString("t")) || hello.optInt("ver")<11) throw new Exception("MacのKnitを更新してください。");
                connected=true; retry=1000; lowLatency(true); permissions();
                status("接続済み",ControlService.current==null ? "画面操作の許可が必要です。アプリで設定してください。" : "Macから操作できます。通知からいつでも停止できます。");
                long mainId=id;
                JSONObject bulkArgs=new JSONObject(credentials.toString());
                String address=credentials.getString("address"); int colon=address.lastIndexOf(':');
                bulkArgs.put("address",address.substring(0,colon+1)+(Integer.parseInt(address.substring(colon+1))+2));
                bulkArgs.put("bulk",true).put("dir",new File(getFilesDir(),"received").getAbsolutePath());
                new Thread(()->bulkLoop(generation,mainId,bulkArgs),"Knit file connection").start();
                while(valid(generation) && handle==id) {
                    JSONObject msg=(JSONObject)Native.request(3,id,Native.obj(),null);
                    if("ping".equals(msg.optString("t"))) { sendDirect(id,Native.obj("t","pong","ts",msg.optLong("ts"))); continue; }
                    if("bye".equals(msg.optString("t"))) break;
                    long source=id;
                    if(coalesceMove(msg,generation,source)) continue;
                    moves=null;
                    ui.post(()-> { if(valid(generation) && handle==source) dispatch(msg); });
                }
            } catch(Exception e) { if(valid(generation)) status("再接続を待機",safe(e)); }
            finally {
                Native.close(id);
                if(handle==id) { handle=0; connected=false; lowLatency(false); long bulk=bulkHandle; bulkHandle=0; Native.close(bulk); ui.post(()->{ if(handle==0&&valid(generation)){if(ControlService.current!=null) ControlService.current.release();if(KnitIme.current!=null)KnitIme.current.release();} }); }
            }
            if(valid(generation)) try { Thread.sleep(retry); retry=Math.min(15000,retry*2); } catch(InterruptedException e) { break; }
        }
    }
    static String safe(Exception e) { String m=e.getMessage(); return m!=null && m.length()<180 ? m : "接続を処理できませんでした。ネットワークと登録を確認してください。"; }
    // Wi-Fi delivers pointer input in bursts. Consecutive moves merge into one UI task, so a burst of
    // thirty packets costs one dispatch. Any other message seals the batch, which keeps clicks ordered.
    private static final class Moves { boolean sealed,abs; double nx,ny,dx,dy; }
    private Moves moves;
    private boolean coalesceMove(JSONObject msg,long generation,long source) {
        String t=msg.optString("t");
        boolean absolute=t.equals("mouse_abs")||t.equals("warp");
        if(!absolute&&!t.equals("mouse_move")) return false;
        double a=absolute?msg.optDouble("nx"):msg.optDouble("dx"),b=absolute?msg.optDouble("ny"):msg.optDouble("dy");
        if(!Double.isFinite(a)||!Double.isFinite(b)) return true;
        Moves m=moves;
        if(m!=null) synchronized(m) { if(!m.sealed) { merge(m,absolute,a,b); return true; } }
        m=new Moves(); merge(m,absolute,a,b); moves=m;
        final Moves batch=m;
        ui.post(()-> {
            boolean abs; double nx,ny,dx,dy;
            synchronized(batch) { batch.sealed=true; abs=batch.abs; nx=batch.nx; ny=batch.ny; dx=batch.dx; dy=batch.dy; }
            if(!valid(generation)||handle!=source) return;
            if(abs) dispatch(Native.obj("t","mouse_abs","nx",nx,"ny",ny));
            if(dx!=0||dy!=0) dispatch(Native.obj("t","mouse_move","dx",dx,"dy",dy));
        });
        return true;
    }
    private static void merge(Moves m,boolean absolute,double a,double b) {
        if(absolute) { m.abs=true; m.nx=a; m.ny=b; m.dx=m.dy=0; }
        else { m.dx+=Math.max(-500,Math.min(500,a)); m.dy+=Math.max(-500,Math.min(500,b)); }
    }
    private void dispatch(JSONObject msg) {
        String t=msg.optString("t");
        if(t.equals("selected")) { selected=msg.optBoolean("on"); if(!selected) {long old=bulkHandle;bulkHandle=0;Native.close(old);if(ControlService.current!=null)ControlService.current.release();if(KnitIme.current!=null)KnitIme.current.release();} return; }
        if(t.equals("cfg")) { selected=true;clip=msg.optBoolean("clip",true); if(clip) Sharing.applyPending(this); return; }
        if(t.equals("clip")) { if(clip && msg.optString("text").length()<=1024*1024) Sharing.receiveText(this,msg.optString("text")); return; }
        if(t.equals("leave")) { if(ControlService.current!=null) ControlService.current.release(); if(KnitIme.current!=null) KnitIme.current.release(); return; }
        if(!unlocked()) { if(ControlService.current!=null) ControlService.current.release();if(KnitIme.current!=null)KnitIme.current.release(); return; }
        if(t.equals("open_url")) { Sharing.receiveUrl(this,msg.optString("url")); return; }
        if(t.equals("key") || t.equals("text") || t.equals("ime")) {
            if(KnitIme.current!=null) KnitIme.current.receive(msg); else if(t.equals("key") && msg.optBoolean("down")) report("文字入力にはKnitキーボードを選択してください。");
            return;
        }
        if(ControlService.current!=null) ControlService.current.receive(msg);
    }
    private void bulkLoop(long generation,long mainId,JSONObject args) {
        while(valid(generation)&&handle==mainId) {
            long id=0;
            try {
                if(!selected) {Thread.sleep(500);continue;}
                id=((JSONObject)Native.request(2,0,args,null)).getLong("handle");
                if(!valid(generation) || handle!=mainId || !selected) {Native.close(id);continue;}
                bulkHandle=id;final long bulk=id;
                // つながる前に共有されていたファイルがあれば、ここで自動送信する
                JSONArray pending=pendingFiles.getAndSet(null);
                if(pending!=null)sendFiles(pending);
                ScheduledExecutorService heartbeat=Executors.newSingleThreadScheduledExecutor();
                heartbeat.scheduleWithFixedDelay(()-> {if(valid(generation)&&handle==mainId&&selected)try{Native.request(8,bulk,Native.obj(),null);}catch(Exception ignored){Native.close(bulk);}},10,10,TimeUnit.SECONDS);
                try {
                    while(valid(generation)&&handle==mainId&&selected) {
                        JSONObject event=(JSONObject)Native.request(6,id,Native.obj("dir",args.getString("dir")),null);
                        if(valid(generation)&&handle==mainId&&selected)Sharing.receiveFiles(this,event);
                    }
                }finally{heartbeat.shutdownNow();}
            }catch(Exception ignored) {if(valid(generation)&&handle==mainId&&selected)report("ファイル用の接続を再試行しています。");}
            finally{Native.close(id);if(bulkHandle==id)bulkHandle=0;}
            if(valid(generation)&&handle==mainId)try{Thread.sleep(2000);}catch(InterruptedException e){break;}
        }
    }
    void sendFiles(JSONArray paths) {
        long id=bulkHandle;
        if(id==0) { pendingFiles.set(paths); report("Macがこのタブレットを選択すると送信されます(Macの画面の端へカーソルを移動してください)。"); return; }
        files.execute(()-> { try { Native.request(7,id,Native.obj("paths",paths),null); report("Macへファイルを送信しました。"); } catch(Exception e) { report(safe(e)); }
            finally { for(int i=0;i<paths.length();i++){File f=new File(paths.optString(i));try{if(f.getCanonicalPath().startsWith(new File(getCacheDir(),"send").getCanonicalPath()+File.separator)){f.delete();f.getParentFile().delete();}}catch(Exception ignored){}} }
        });
    }
    static void send(JSONObject msg) {
        ConnectionService service=current;
        if(service==null || !connected) return;
        long id=service.handle;
        try { service.sender.execute(()-> { if(service.running && service.handle==id) try { sendDirect(id,msg); } catch(Exception ignored) { Native.close(id); } }); } catch(RejectedExecutionException ignored) {}
    }
    private static void sendDirect(long id,JSONObject msg) throws Exception { Native.request(4,id,msg,null); }
    Point screen() { Point p=new Point(); ((WindowManager)getSystemService(WINDOW_SERVICE)).getDefaultDisplay().getRealSize(p); return p; }
    void permissions() {
        lastControl=ControlService.current!=null&&unlocked();lastKeyboard=KnitIme.selected(this);lastJapanese=KnitIme.japaneseReady();
        DisplayMetrics m=new DisplayMetrics(); ((WindowManager)getSystemService(WINDOW_SERVICE)).getDefaultDisplay().getRealMetrics(m);
        Point p=screen();
        send(Native.obj("t","screen","w",p.x,"h",p.y));
        double x=m.xdpi,y=m.ydpi;
        send(Native.obj("t","tablet_info","width_mm",x>=50&&x<=1200?p.x/x*25.4:0,"height_mm",y>=50&&y<=1200?p.y/y*25.4:0,"control",lastControl,"keyboard",lastKeyboard,"japanese",lastJapanese));
    }
    @Override public void onConfigurationChanged(android.content.res.Configuration config) { super.onConfigurationChanged(config); if(ControlService.current!=null) ControlService.current.release(); permissions(); }
    @Override public void onDestroy() {
        running=false; lowLatency(false); ui.removeCallbacks(permissionTicker); ++epoch; connected=false; current=null;
        pendingFiles.set(null);
        long main=handle,bulk=bulkHandle; handle=0; bulkHandle=0;
        Native.close(main); Native.close(bulk); sender.shutdownNow(); files.shutdownNow();
        if(ControlService.current!=null) ControlService.current.release();
        if(KnitIme.current!=null) KnitIme.current.release();
        state="停止中"; detail="接続を開始するまでMacからの操作は受け付けません。";
        stopForeground(STOP_FOREGROUND_REMOVE); super.onDestroy();
    }
    @Override public IBinder onBind(Intent intent) { return null; }
}
