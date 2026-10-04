package app.knit;
import android.Manifest;
import android.app.*;
import android.content.*;
import android.content.pm.PackageManager;
import android.graphics.Color;
import android.os.*;
import android.net.Uri;
import android.net.wifi.WifiManager;
import android.provider.Settings;
import android.view.*;
import android.view.inputmethod.InputMethodManager;
import android.widget.*;
import org.json.*;
import java.util.concurrent.*;

public final class MainActivity extends Activity {
    static volatile boolean foreground;
    private final Handler ui=new Handler(Looper.getMainLooper());
    private final ExecutorService worker=Executors.newSingleThreadExecutor();
    private TextView status,detail,control,keyboard,feedback;
    private EditText host,code;
    private Button connect,openUrl,updateButton;
    private TextView updateInfo;
    private LinearLayout candidates;
    private volatile boolean busy;
    private ClipboardManager.OnPrimaryClipChangedListener clipboardListener;
    private final Runnable refresh=new Runnable(){public void run(){update();ui.postDelayed(this,1000);}};
    private int dp(int n){return(int)(getResources().getDisplayMetrics().density*n+.5);}
    private TextView text(LinearLayout parent,String value,int size,boolean bold){TextView t=new TextView(this);t.setText(value);t.setTextSize(size);t.setTextColor(Color.rgb(31,41,63));if(bold)t.setTypeface(null,android.graphics.Typeface.BOLD);LinearLayout.LayoutParams p=new LinearLayout.LayoutParams(-1,-2);p.bottomMargin=dp(10);parent.addView(t,p);return t;}
    private LinearLayout card(LinearLayout parent){LinearLayout c=new LinearLayout(this);c.setOrientation(LinearLayout.VERTICAL);c.setPadding(dp(20),dp(18),dp(20),dp(18));android.graphics.drawable.GradientDrawable bg=new android.graphics.drawable.GradientDrawable();bg.setColor(Color.WHITE);bg.setCornerRadius(dp(18));c.setBackground(bg);LinearLayout.LayoutParams p=new LinearLayout.LayoutParams(-1,-2);p.bottomMargin=dp(16);parent.addView(c,p);return c;}
    private void primary(Button b){b.setBackgroundTintList(android.content.res.ColorStateList.valueOf(Color.rgb(64,95,198)));b.setTextColor(Color.WHITE);}
    private Button button(LinearLayout p,String value,View.OnClickListener action){Button b=new Button(this);b.setText(value);b.setAllCaps(false);b.setOnClickListener(action);p.addView(b,new LinearLayout.LayoutParams(-1,dp(50)));return b;}
    @Override public void onCreate(Bundle state){super.onCreate(state);
        ScrollView scroll=new ScrollView(this);scroll.setFillViewport(true);
        scroll.setOnApplyWindowInsetsListener((v,insets)->{v.setPadding(insets.getSystemWindowInsetLeft(),insets.getSystemWindowInsetTop(),insets.getSystemWindowInsetRight(),insets.getSystemWindowInsetBottom());return insets;});scroll.setBackgroundColor(Color.rgb(245,246,250));
        FrameLayout frame=new FrameLayout(this);scroll.addView(frame,new ScrollView.LayoutParams(-1,-1));
        LinearLayout content=new LinearLayout(this);content.setOrientation(LinearLayout.VERTICAL);content.setPadding(dp(24),dp(28),dp(24),dp(28));FrameLayout.LayoutParams width=new FrameLayout.LayoutParams(Math.min(getResources().getDisplayMetrics().widthPixels,dp(600)),-2,Gravity.TOP|Gravity.CENTER_HORIZONTAL);frame.addView(content,width);setContentView(scroll);
        text(content,"Knit",36,true);text(content,"Macとタブレットを、ひとつの手元で。",16,false);
        LinearLayout summary=card(content);status=text(summary,"未接続",24,true);detail=text(summary,"",14,false);
        connect=button(summary,"接続を開始",v->{if(ConnectionService.current!=null){ConnectionService.setStopped(this,true);stopService(new Intent(this,ConnectionService.class));}else if(new CredentialStore(this).paired()){ConnectionService.setStopped(this,false);if(Build.VERSION.SDK_INT>=33&&checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS)!=PackageManager.PERMISSION_GRANTED)requestPermissions(new String[]{Manifest.permission.POST_NOTIFICATIONS},1);startForegroundService(new Intent(this,ConnectionService.class));}else message("先にMacを登録してください。");update();});primary(connect);
        LinearLayout pairing=card(content);text(pairing,"1  Macを登録",20,true);text(pairing,"MacのKnitの設定「接続」で「端末を登録…」を開くと、自動で見つかります。Macとこの画面に同じ確認番号が出たら、許可します。USBデバッグは不要です。",14,false);
        button(pairing,"近くのMacを探す",v->discover());candidates=new LinearLayout(this);candidates.setOrientation(LinearLayout.VERTICAL);pairing.addView(candidates);
        host=new EditText(this);host.setSingleLine(true);host.setHint("見つからない時だけ: MacのIPアドレス（例: 192.168.1.20）");host.setInputType(1|android.text.InputType.TYPE_TEXT_FLAG_NO_SUGGESTIONS);pairing.addView(host);
        code=new EditText(this);code.setSingleLine(true);code.setHint("自動でつながらない時だけ: Macの6桁コード");code.setInputType(android.text.InputType.TYPE_CLASS_NUMBER);code.setFilters(new android.text.InputFilter[]{new android.text.InputFilter.LengthFilter(12)});pairing.addView(code);
        // 6桁そろい、送り先が決まっていれば、ボタンを押さずに登録を始める
        code.addTextChangedListener(new android.text.TextWatcher(){public void beforeTextChanged(CharSequence c,int a,int b,int d){}public void onTextChanged(CharSequence c,int a,int b,int d){}public void afterTextChanged(android.text.Editable e){if(e.toString().replaceAll("[\\s-]","").length()==6&&host.getText().toString().trim().length()>0)enroll();}});
        Button register=button(pairing,"このMacとつなぐ",v->enroll());primary(register);feedback=text(pairing,"同じWi-Fi・LANなど、双方が通信できるネットワークで使えます。",13,false);
        LinearLayout permission=card(content);text(permission,"2  操作を許可",20,true);control=text(permission,"",14,false);
        button(permission,"画面操作の設定を開く",v->{new AlertDialog.Builder(this).setTitle("Knitの画面操作を許可").setMessage("登録したMacからタップ・ドラッグ・スクロールなどの操作を受け付けます。画面の文字は読み取りません。通知からいつでも停止できます。次の画面で「Knit」または「Knit 画面操作」の使用を有効にしてください。文字入力用のキーボードとは別の設定です。").setPositiveButton("設定へ",(d,w)->openControlSettings()).setNegativeButton("あとで",null).show();});
        text(permission,"一覧が開いた場合は「ダウンロードしたアプリ」内のKnitを選びます。制限付き設定の表示が出た時は、端末の案内を確認してください。",13,false);
        keyboard=text(permission,"",14,false);button(permission,"Knitキーボードを有効にする",v->startActivity(new Intent(Settings.ACTION_INPUT_METHOD_SETTINGS)));button(permission,"使うキーボードを選択",v->((InputMethodManager)getSystemService(INPUT_METHOD_SERVICE)).showInputMethodPicker());
        text(permission,"文字入力をする時はKnitキーボードを選びます。Macの「かな」で日本語、「英数」で英数字に切り替えます。未確定文字はこのタブレットの入力欄に表示され、Spaceで変換・Enterで確定できます。候補もタブレット上で選べます。端末のキーボードはいつでも戻せます。",13,false);
        LinearLayout sharing=card(content);text(sharing,"3  コピーと共有",20,true);text(sharing,"MacからのファイルはDownloads/Knit、画像はPictures/Knitへ保存します。タブレットからはアプリの「共有」でKnitを選べます。",14,false);
        button(sharing,"タブレットのコピーをMacへ送る",v->Sharing.sendClipboard(this));openUrl=button(sharing,"受け取ったURLを開く",v->{if(Sharing.pendingUrl!=null){try{startActivity(new Intent(Intent.ACTION_VIEW,Uri.parse(Sharing.pendingUrl)));Sharing.pendingUrl=null;}catch(ActivityNotFoundException e){message("ブラウザが見つかりませんでした。");}}});
        LinearLayout upd=card(content);text(upd,"アップデート",18,true);updateInfo=text(upd,"",14,false);updateButton=button(upd,"アップデートを確認",v->{if(Updater.phase==Updater.Phase.AVAILABLE)Updater.install(this);else Updater.check(this,false);update();});
        LinearLayout manage=card(content);text(manage,"この端末での管理",18,true);button(manage,"登録を解除",v->new AlertDialog.Builder(this).setTitle("この端末の登録を解除").setMessage("接続を停止し、この端末に保存した接続キーを削除します。Mac側の登録と、他の端末との接続キーは変更されません。").setNegativeButton("戻る",null).setPositiveButton("解除",(d,w)->{stopService(new Intent(this,ConnectionService.class));try{new CredentialStore(this).forget();message("登録を解除しました。");}catch(Exception e){message("登録を解除できませんでした。");}update();}).show());
        text(manage,"試作版 0.1 · Android 10以降\n画面ミラーリングと、タブレットの音をMacで鳴らす機能は、このアプリ版にはまだ含まれません。",12,false);
        clipboardListener=()->Sharing.sendClipboard(this);((ClipboardManager)getSystemService(CLIPBOARD_SERVICE)).addPrimaryClipChangedListener(clipboardListener);
        if(new CredentialStore(this).paired()) message("登録済みです。接続を開始して、操作の許可を確認してください。Macを変更する時は再登録できます。");
        handleShare(getIntent());
        // 未登録なら、開いてすぐ近くのMacを探す
        if(!new CredentialStore(this).paired())discover();
    }
    private void message(String message){feedback.setText(message);}
    private void update(){if(status==null)return;boolean running=ConnectionService.current!=null;boolean paired=new CredentialStore(this).paired();status.setText(running?ConnectionService.state:(paired?"登録済み · 未接続":"Mac未登録"));detail.setText(running?ConnectionService.detail:(paired?"「接続を開始」でWi-Fi・LAN経由の接続を始めます。":"MacのKnitの設定「接続」で「端末を登録…」を開き、この画面で確認してください。"));connect.setText(running?"接続を停止":"接続を開始");control.setText(ControlService.current!=null?"✓ 画面操作は許可されています":"画面操作 · 許可が必要です");keyboard.setText(KnitIme.selected(this)?"✓ Knitキーボードを選択中":"文字入力 · Knitキーボードを選択してください");openUrl.setEnabled(Sharing.pendingUrl!=null);
        Updater.expireInstallIfStuck();
        Updater.Phase phase=Updater.phase;updateInfo.setText("現在の版 "+Updater.current(this)+(Updater.message.isEmpty()?"":"\n"+Updater.message));
        updateButton.setEnabled(phase==Updater.Phase.IDLE||phase==Updater.Phase.AVAILABLE);
        updateButton.setText(phase==Updater.Phase.AVAILABLE?"Knit "+Updater.availableVersion()+" に更新":phase==Updater.Phase.CHECKING?"確認しています…":phase==Updater.Phase.DOWNLOADING?"ダウンロードしています…":phase==Updater.Phase.INSTALLING?"確認画面を待っています…":"アップデートを確認");}
    private void openControlSettings(){
        // Android's settings app exposes this optional detail action. It only
        // opens the consent page; the user enables the service themselves.
        Intent details=new Intent("android.settings.ACCESSIBILITY_DETAILS_SETTINGS")
                .putExtra("android.intent.extra.COMPONENT_NAME",new ComponentName(this,ControlService.class));
        try{startActivity(details);}
        catch(ActivityNotFoundException|SecurityException unavailable){
            try{startActivity(new Intent(Settings.ACTION_ACCESSIBILITY_SETTINGS));}
            catch(ActivityNotFoundException|SecurityException missing){message("端末の設定からユーザー補助 → ダウンロードしたアプリ → Knitを開いてください。");}
        }
    }
    private void discover(){if(busy)return;busy=true;message("近くのMacを探しています…");worker.execute(()->{
        WifiManager.MulticastLock lock=((WifiManager)getApplicationContext().getSystemService(WIFI_SERVICE)).createMulticastLock("Knit discovery");lock.setReferenceCounted(false);
        try{lock.acquire();JSONArray peers=(JSONArray)Native.request(0,0,Native.obj(),null);ui.post(()->{if(isDestroyed())return;candidates.removeAllViews();for(int i=0;i<peers.length();i++){JSONObject p=peers.optJSONObject(i);String address=p.optString("address");String ip=address.substring(0,address.lastIndexOf(':'));button(candidates,p.optString("name")+" · "+ip,v->host.setText(ip));}
            // 1台だけ見つかった時は、選ぶ手間を省いて自動で入れる
            String only=null;
            if(peers.length()==1){JSONObject o=peers.optJSONObject(0);String a=o.optString("address");only=a.substring(0,a.lastIndexOf(':'));host.setText(only);}
            message(peers.length()==0?"Macの設定「接続」で「端末を登録…」を開いてください。見つからない場合はIPアドレスを入力できます。":peers.length()==1?"Macが見つかりました。確認番号を出しています…":"つなぐMacを選んでください。");
            // 1台だけなら、選ぶ手間なしで確認を求める(持ち主が許可するまで何も保存されない)
            if(only!=null){final String ip1=only;busy=false;approve(ip1);}});}catch(Exception e){ui.post(()->{if(!isDestroyed())message(ConnectionService.safe(e));});}finally{if(lock.isHeld())lock.release();busy=false;}
        });}
    private void enroll(){if(busy)return;String ip=host.getText().toString().trim(),pin=code.getText().toString().replaceAll("[\\s-]","");if(pin.isEmpty()){if(ip.isEmpty()){message("近くのMacを探すか、MacのIPアドレスを入力してください。");}else{approve(ip);}return;}if(pin.length()!=6){message("6桁のコードを入力してください。");return;}if(ConnectionService.current!=null){message("接続を停止してから登録してください。");return;}busy=true;message("Macへ登録しています…");worker.execute(()->{try{Native.request(1,0,Native.obj("address",ip,"code",pin),new CredentialStore(this));ui.post(()->{if(!isDestroyed()){code.setText("");message("登録しました。「接続を開始」を押してください。");update();}});}catch(Exception e){ui.post(()->{if(!isDestroyed())message(ConnectionService.safe(e));});}finally{busy=false;}});}
    private volatile long pendingHandle;
    /** 承認方式: Macに「つなぎたい」と伝え、Macの画面に出る番号を4つの候補から選んでもらう(選ぶまで何も保存しない) */
    private void approve(String ip){
        if(busy)return;if(ConnectionService.current!=null){message("接続を停止してから登録してください。");return;}
        busy=true;message("Macに確認を求めています…");
        worker.execute(()->{try{
            JSONObject r=(JSONObject)Native.request(11,0,Native.obj("address",ip,"name",Build.MODEL),null);
            final long handle=r.getLong("handle");final String sas=r.getString("sas"),server=r.optString("server","Mac"),peer=r.optString("peer",ip);
            final JSONArray list=r.getJSONArray("choices");final String[] choices=new String[list.length()];for(int i=0;i<choices.length;i++)choices[i]=list.optString(i);
            pendingHandle=handle;
            ui.post(()->{
                if(isDestroyed()){finishApproval(handle,false,null);return;}
                message("Macの画面に出ている番号を選んでください。");
                new AlertDialog.Builder(this).setTitle("Mac「"+server+"」("+peer+")の画面に出ている番号は?").setItems(choices,(d,which)->{
                    // 本物の番号を選んだ時だけ進める。違えば取り消す
                    if(choices[which].equals(sas))finishApproval(handle,true,null);else finishApproval(handle,false,"番号が違います。Macの画面を確かめて、もう一度やり直してください。");
                }).setNegativeButton("やめる（心当たりがない）",(d,w)->finishApproval(handle,false,null)).setCancelable(false).show();
            });
        }catch(Exception e){ui.post(()->{if(!isDestroyed())message(ConnectionService.safe(e));});busy=false;}});
    }
    private void finishApproval(long handle,boolean ok,String note){
        pendingHandle=0;
        worker.execute(()->{try{
            Native.request(12,handle,Native.obj("approve",ok),ok?new CredentialStore(this):null);
            ui.post(()->{if(!isDestroyed()){code.setText("");message(ok?"登録しました。「接続を開始」を押してください。":(note!=null?note:"やめました。"));update();}});
        }catch(Exception e){ui.post(()->{if(!isDestroyed())message(ConnectionService.safe(e));});}finally{busy=false;}});
    }
    private void handleShare(Intent intent){
        // SEND(1件)とSEND_MULTIPLE(複数枚)の両方を受け付ける。対象は
        // 画像・動画・音声・テキスト・ファイル(マニフェストのintent-filter参照)。
        // ファイル送信経路は種類に依存しないため、種類の制限はここではしない
        boolean single=Intent.ACTION_SEND.equals(intent.getAction()),multiple=Intent.ACTION_SEND_MULTIPLE.equals(intent.getAction());
        if(!single&&!multiple)return;
        java.util.ArrayList<Uri> uris=new java.util.ArrayList<>();
        if(single){Uri uri=intent.getParcelableExtra(Intent.EXTRA_STREAM);if(uri!=null)uris.add(uri);}
        else{java.util.ArrayList<android.os.Parcelable> list=intent.getParcelableArrayListExtra(Intent.EXTRA_STREAM);if(list!=null)for(android.os.Parcelable p:list)if(p instanceof Uri)uris.add((Uri)p);}
        String text=intent.getStringExtra(Intent.EXTRA_TEXT);
        if(!ConnectionService.connected){
            // 共有されたファイルは破棄せず持っておく。接続してMacがこの端末を
            // 選んだ時点で自動送信される(接続が先か共有が先かを意識しなくてよい)
            if(uris.isEmpty()){ui.post(()->message("Macへ接続してから、もう一度共有してください。「接続を開始」でつながると、近くのMacは自動で見つかります。"));return;}
            final int total=uris.size();
            worker.execute(()->{try{
                // 受け取り時点で上限を検査(本体のドラッグ送信と同じ512件・合計10GiB)。
                // 超過はコピーを始める前に断る(サイズ問い合わせは取れない物を除外しない)
                String over=ShareLimits.reject(uris.size(),ShareLimits.totalBytes(this,uris));
                if(over!=null){ui.post(()->{if(!isDestroyed())message(over);});return;}
                java.util.ArrayList<String> paths=new java.util.ArrayList<>();
                for(Uri u:uris)paths.add(Sharing.prepareUpload(this,u).getAbsolutePath());
                // 未接続での積み込みは新しい共有のため、再送済みの印を戻す
                //(失敗時の 1 回だけ保留戻しをこの共有にも適用できるように)
                ConnectionService.pendingFiles.set(new JSONArray(paths));
                ConnectionService.pendingFilesRetried.set(false);
                ui.post(()->{if(!isDestroyed())message("Macへ接続すると、"+total+"件を自動で送ります。「接続を開始」を押してください。");});
            }catch(Exception e){ui.post(()->{if(!isDestroyed())message(ConnectionService.safe(e));});}});
            return;
        }
        if(text!=null){ConnectionService.send(Native.obj("t","clip","text",text));ui.post(()->message("Macへテキストを送りました。"));}
        if(uris.isEmpty())return;
        final int total=uris.size();
        worker.execute(()->{try{
            // 接続中の共有も同じ上限で検査してからコピーする
            String over=ShareLimits.reject(uris.size(),ShareLimits.totalBytes(this,uris));
            if(over!=null){ui.post(()->{if(!isDestroyed())message(over);});return;}
            java.util.ArrayList<String> paths=new java.util.ArrayList<>();
            for(Uri u:uris)paths.add(Sharing.prepareUpload(this,u).getAbsolutePath());
            ConnectionService service=ConnectionService.current;
            if(service!=null)service.sendFiles(new JSONArray(paths));
            else ui.post(()->{if(!isDestroyed())message("Macへ接続してから、もう一度共有してください。");});
        }catch(Exception e){ui.post(()->{if(!isDestroyed())message(ConnectionService.safe(e));});}});
    }
    @Override protected void onNewIntent(Intent intent){super.onNewIntent(intent);setIntent(intent);handleShare(intent);}
    @Override protected void onResume(){super.onResume();foreground=true;ConnectionService.autoStart(this);Sharing.applyPending(this);Updater.autoCheck(this);ui.post(refresh);if(ConnectionService.current!=null)ConnectionService.current.permissions();}
    @Override protected void onPause(){foreground=false;ui.removeCallbacks(refresh);super.onPause();}
    @Override protected void onDestroy(){long h=pendingHandle;if(h!=0){pendingHandle=0;try{Native.request(12,h,Native.obj("approve",false),null);}catch(Exception ignored){}}ui.removeCallbacks(refresh);((ClipboardManager)getSystemService(CLIPBOARD_SERVICE)).removePrimaryClipChangedListener(clipboardListener);worker.shutdownNow();super.onDestroy();}
}
