package app.knit;
import android.accessibilityservice.*;
import android.view.accessibility.AccessibilityEvent;
import android.content.*;
import android.graphics.*;
import android.os.*;
import android.view.*;
import android.widget.FrameLayout;
import org.json.*;
import java.util.ArrayList;

public final class ControlService extends AccessibilityService {
    @android.annotation.SuppressLint("StaticFieldLeak") // Active bound service only; cleared in onDestroy.
    static ControlService current;
    private WindowManager wm;
    private View cursor;
    private FrameLayout layer;
    private WindowManager.LayoutParams overlay;
    private float x=200,y=200,downX,downY;
    private boolean active,held,busy,ending,pinching;
    private float radius=60,targetRadius=60;
    private GestureDescription.StrokeDescription stroke,second;
    private float endX,endY;
    private long pressedAt,generation;
    private final Handler ui=new Handler(Looper.getMainLooper());
    // カーソルは OS の標準矢印(AOSP の公式素材 pointer_arrow を同梱)をそのまま描く。
    // 自前の形は作らない。大きさは素材の intrinsic(24dp 相当・密度別の res で自動対応)。
    // ホットスポットは OS 定義どおり 4.5dp/3.5dp
    private android.graphics.drawable.Drawable arrow;
    private float hs,hsy;
    private final ArrayList<String> apps=new ArrayList<>();
    private String foreground="";
    @android.annotation.SuppressLint("RtlHardcoded") // Absolute physical screen coordinates, including RTL devices.
    @Override protected void onServiceConnected() {
        current=this; wm=(WindowManager)getSystemService(WINDOW_SERVICE);
        // カーソルは OS の標準矢印(AOSP の公式素材 pointer_arrow を同梱)をそのまま描く。
        // 自前の形は作らない。大きさは素材の intrinsic(24dp 相当・密度別の res で自動対応)。
        // ホットスポットは OS 定義どおり 4.5dp/3.5dp
        float dens=getResources().getDisplayMetrics().density;
        hs=4.5f*dens; hsy=3.5f*dens;
        arrow=getDrawable(R.drawable.pointer_arrow);
        if(arrow!=null && arrow.getIntrinsicWidth()>0) arrow.setBounds(0,0,arrow.getIntrinsicWidth(),arrow.getIntrinsicHeight());
        cursor=new View(this) { private final Paint p=new Paint(3); @Override protected void onDraw(Canvas c) { if(arrow!=null) arrow.draw(c); } };
        int cs=arrow!=null&&arrow.getIntrinsicWidth()>0?arrow.getIntrinsicWidth():(int)(13f*dens*2);
        layer=new FrameLayout(this); layer.addView(cursor,new FrameLayout.LayoutParams(cs,cs));
        overlay=new WindowManager.LayoutParams(WindowManager.LayoutParams.MATCH_PARENT,WindowManager.LayoutParams.MATCH_PARENT,WindowManager.LayoutParams.TYPE_ACCESSIBILITY_OVERLAY,WindowManager.LayoutParams.FLAG_NOT_FOCUSABLE|WindowManager.LayoutParams.FLAG_NOT_TOUCHABLE|WindowManager.LayoutParams.FLAG_LAYOUT_IN_SCREEN|WindowManager.LayoutParams.FLAG_LAYOUT_NO_LIMITS|WindowManager.LayoutParams.FLAG_HARDWARE_ACCELERATED,PixelFormat.TRANSLUCENT);
        layer.setVisibility(View.GONE); wm.addView(layer,overlay); Point size=screen(); sw=size.x; sh=size.y;
        if(ConnectionService.current!=null) ConnectionService.current.permissions();
    }
    private Point screen() { Point p=new Point(); wm.getDefaultDisplay().getRealSize(p); return p; }
    private float clamp(float n,int limit) { return Math.max(1,Math.min(limit-2,n)); }
    private boolean allowed() { return current==this && ConnectionService.connected && ConnectionService.current!=null && ConnectionService.current.unlocked(); }
    private int sw=1,sh=1;
    private boolean framePending;
    private float rx,ry;
    private long lastFrame;
    // The pointer eases toward the newest position once per display frame. Wi-Fi delivers input
    // unevenly, and this hides the unevenness; the window is never moved, only the view's translation.
    private final Choreographer.FrameCallback frame=t-> {
        framePending=false; if(current!=this||cursor==null) return;
        float dt=lastFrame==0?16f:Math.min(50f,(t-lastFrame)/1e6f); lastFrame=t;
        float k=1f-(float)Math.exp(-dt/8f);
        float dx=x-rx,dy=y-ry;
        if(Math.abs(dx)>400||Math.abs(dy)>400||(Math.abs(dx)<.4f&&Math.abs(dy)<.4f)) { rx=x; ry=y; } else { rx+=dx*k; ry+=dy*k; }
        cursor.setTranslationX(rx-hs); cursor.setTranslationY(ry-hsy);
        if(held) continueDrag();
        if(active&&(rx!=x||ry!=y)) schedule(); else lastFrame=0;
    };
    private void schedule() { if(framePending) return; framePending=true; Choreographer.getInstance().postFrameCallback(frame); }
    private void position() {
        if(layer==null) return;
        Point p=screen(); sw=p.x; sh=p.y; x=clamp(x,sw); y=clamp(y,sh);
        if(!active) { rx=x; ry=y; cursor.setTranslationX(rx-hs); cursor.setTranslationY(ry-hsy); }
        layer.setVisibility(active?View.VISIBLE:View.GONE);
    }
    private void moved() {
        if(!active) { active=true; position(); }
        x=clamp(x,sw); y=clamp(y,sh); schedule();
    }
    void receive(JSONObject m) {
        if(!allowed()) { release(); return; }
        String t=m.optString("t");
        switch(t) {
            case "warp": case "mouse_abs": {
                double nx=m.optDouble("nx"),ny=m.optDouble("ny");
                if(!Double.isFinite(nx)||!Double.isFinite(ny)) return;
                Point p=screen(); x=(float)nx*p.x; y=(float)ny*p.y; sw=p.x; sh=p.y; moved(); break;
            }
            case "mouse_move": {
                double dx=m.optDouble("dx"),dy=m.optDouble("dy");
                if(!Double.isFinite(dx)||!Double.isFinite(dy)) return;
                x+=(float)Math.max(-500,Math.min(500,dx)); y+=(float)Math.max(-500,Math.min(500,dy)); moved(); break;
            }
            case "mouse_btn": button(m.optInt("btn"),m.optBoolean("down")); break;
            case "scroll": scroll(m.optDouble("dx"),m.optDouble("dy")); break;
            case "pinch": pinch(m.optDouble("delta"),m.optInt("phase")); break;
            case "tablet_gesture": action(m.optString("action")); break;
        }
    }
    private Path path(float sx,float sy,float tx,float ty) { Path p=new Path(); p.moveTo(sx,sy); if(sx!=tx||sy!=ty) p.lineTo(tx,ty); return p; }
    private void button(int button,boolean down) {
        if(button==1 && down) { longPress(); return; }
        if(button==3 && down) { action("back"); return; }
        if(button!=0) return;
        if(down && !held && !busy) {
            held=true; active=true; pressedAt=SystemClock.uptimeMillis(); downX=x;downY=y; ending=false;
            stroke=new GestureDescription.StrokeDescription(path(x,y,x,y),0,32,true);
            endX=x;endY=y; submit(stroke,null);
        } else if(!down && held) { held=false; ending=true; continueDrag(); }
    }
    private void submit(GestureDescription.StrokeDescription a,GestureDescription.StrokeDescription b) {
        if(current!=this) {clearTouch();return;}
        busy=true; long epoch=generation;
        GestureDescription.Builder builder=new GestureDescription.Builder().addStroke(a); if(b!=null) builder.addStroke(b);
        if(!dispatchGesture(builder.build(),new GestureResultCallback() {
            @Override public void onCompleted(GestureDescription g) { if(epoch!=generation) return; busy=false; if(held||ending) continueDrag(); else if(pinching) continuePinch(); else if(scrolling) continueScroll(); }
            @Override public void onCancelled(GestureDescription g) { if(epoch!=generation) return; clearTouch(); ConnectionService.report("操作が中断されました。もう一度操作してください。"); }
        },ui)) clearTouch();
    }
    private void continueDrag() {
        if(busy||stroke==null||pinching) return;
        boolean finish=ending||!held;
        float tx=finish?x:x,ty=y;
        if(!finish && tx==endX&&ty==endY) return;
        GestureDescription.StrokeDescription next=stroke.continueStroke(path(endX,endY,tx,ty),0,32,!finish);
        endX=tx;endY=ty;stroke=next;
        if(finish) { ending=false;stroke=null; }
        submit(next,null);
        // A continued stationary stroke needs no repeated dispatch: Android keeps
        // the touch down until a continuation ends it or another gesture cancels it.
    }
    private void longPress() { if(busy||held||pinching) return; submit(new GestureDescription.StrokeDescription(path(x,y,x,y),0,650),null); }
    private double scrollX,scrollY;
    private boolean scrolling,scrollPoll;
    private GestureDescription.StrokeDescription scrollStroke;
    private float fx,fy;
    private long lastScrollAt;
    // One finger stays down for the whole scroll and follows the input, so content tracks it without the
    // lift-and-replace hitch of one short swipe per batch. It lifts once input has been idle.
    private void scroll(double dx,double dy) {
        if(!Double.isFinite(dx)||!Double.isFinite(dy)||held||pinching) return;
        scrollX=Math.max(-10,Math.min(10,scrollX+dx));scrollY=Math.max(-10,Math.min(10,scrollY+dy));
        lastScrollAt=SystemClock.uptimeMillis();
        if(scrolling) { continueScroll(); return; }
        if(busy) { pollScroll(16); return; }
        fx=x;fy=y;scrolling=true;
        scrollStroke=new GestureDescription.StrokeDescription(path(fx,fy,fx,fy),0,16,true);
        submit(scrollStroke,null);
    }
    private void pollScroll(long delay) {
        if(scrollPoll) return; scrollPoll=true; long epoch=generation;
        ui.postDelayed(()-> { scrollPoll=false; if(epoch!=generation||!allowed()) {scrollX=scrollY=0;return;} if(scrolling) continueScroll(); else if(scrollX!=0||scrollY!=0) scroll(0,0); },delay);
    }
    private void continueScroll() {
        if(busy||!scrolling||scrollStroke==null) return;
        Point p=screen();
        float rawX=fx-(float)scrollX*48,rawY=fy-(float)scrollY*48;
        float tx=Math.max(24,Math.min(p.x-24,rawX)),ty=Math.max(24,Math.min(p.y-24,rawY));
        boolean edge=Math.abs(tx-rawX)>1||Math.abs(ty-rawY)>1;
        boolean idle=SystemClock.uptimeMillis()-lastScrollAt>=120;
        scrollX=scrollY=0;
        if(!idle&&!edge&&tx==fx&&ty==fy) { pollScroll(24); return; }
        boolean finish=idle||edge;
        GestureDescription.StrokeDescription next=scrollStroke.continueStroke(path(fx,fy,tx,ty),0,16,!finish);
        fx=tx;fy=ty;scrollStroke=next;
        if(finish) { scrolling=false;scrollStroke=null; }
        submit(next,null);
    }
    private void pinch(double delta,int phase) {
        if(!Double.isFinite(delta)||phase<0||phase>3||held) return;
        Point p=screen();
        if(phase==0 && !busy) { radius=Math.max(12,Math.min(70,Math.min(Math.min(x,p.x-x),Math.min(y,p.y-y))-2)); targetRadius=radius; pinching=true; ending=false; stroke=new GestureDescription.StrokeDescription(path(x-radius,y,x-radius,y),0,32,true);second=new GestureDescription.StrokeDescription(path(x+radius,y,x+radius,y),0,32,true);submit(stroke,second); }
        if(!pinching) return;
        float max=Math.max(12,Math.min(Math.min(x,p.x-x),Math.min(y,p.y-y))-2);
        targetRadius=Math.max(12,Math.min(max,targetRadius*(float)(1+Math.max(-.8,Math.min(.8,delta)))));
        if(phase==2||phase==3) ending=true;
        continuePinch();
    }
    private void continuePinch() {
        if(busy||!pinching||stroke==null||second==null) return;
        if(!ending && Math.abs(radius-targetRadius)<.1) return;
        boolean finish=ending;
        GestureDescription.StrokeDescription a=stroke.continueStroke(path(x-radius,y,x-targetRadius,y),0,32,!finish);
        GestureDescription.StrokeDescription b=second.continueStroke(path(x+radius,y,x+targetRadius,y),0,32,!finish);
        radius=targetRadius;stroke=a;second=b;
        if(finish) {pinching=false;ending=false;stroke=second=null;}
        submit(a,b);
    }
    private void action(String action) {
        release();
        int operation;
        switch(action) {
            case "back": operation=GLOBAL_ACTION_BACK;break;
            case "home": operation=GLOBAL_ACTION_HOME;break;
            case "recents": operation=GLOBAL_ACTION_RECENTS;break;
            case "screenshot": operation=GLOBAL_ACTION_TAKE_SCREENSHOT;break;
            case "previous_app": case "next_app": switchApp(action.equals("previous_app")?-1:1);return;
            default:return;
        }
        if(!performGlobalAction(operation)) ConnectionService.report("この画面では操作できません。");
    }
    private void switchApp(int direction) {
        if(apps.size()<2) { performGlobalAction(GLOBAL_ACTION_RECENTS); ConnectionService.report("アプリ履歴がまだないため、最近のタスクを開きました。"); return; }
        int index=apps.indexOf(foreground); int next=Math.floorMod(index+direction,apps.size());
        Intent intent=getPackageManager().getLaunchIntentForPackage(apps.get(next));
        if(intent==null) { performGlobalAction(GLOBAL_ACTION_RECENTS); return; }
        try { startActivity(intent.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)); } catch(Exception ignored) { ConnectionService.report("アプリを切り替えられませんでした。"); }
    }
    private void clearTouch() { busy=held=ending=pinching=scrolling=false;stroke=second=scrollStroke=null; }
    void release() {
        // Finish a held continuation before hiding. If a callback is pending, it
        // drains the final segment; do not queue a new unrelated tap to cancel.
        if(held) { held=false;ending=true;continueDrag(); }
        if(pinching) {ending=true;continuePinch();}
        if(scrolling) { lastScrollAt=0; continueScroll(); }
        active=false;scrollX=scrollY=0;position();
    }
    @Override public void onAccessibilityEvent(AccessibilityEvent event) {
        if(event.getEventType()!=AccessibilityEvent.TYPE_WINDOW_STATE_CHANGED || event.getPackageName()==null) return;
        String pkg=event.getPackageName().toString();
        if(pkg.equals(getPackageName()) || pkg.startsWith("com.android.systemui")) return;
        foreground=pkg;if(!apps.contains(pkg)) {apps.add(pkg);if(apps.size()>20)apps.remove(0);}
    }
    @Override public void onInterrupt() { release(); }
    @Override public void onDestroy() { ++generation;clearTouch();if(layer!=null)wm.removeView(layer);layer=null;cursor=null;current=null;Choreographer.getInstance().removeFrameCallback(frame);framePending=false;if(ConnectionService.current!=null)ConnectionService.current.permissions();super.onDestroy(); }
}
