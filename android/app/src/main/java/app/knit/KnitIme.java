package app.knit;
import android.inputmethodservice.InputMethodService;
import android.view.*;
import android.view.KeyEvent;
import android.view.inputmethod.*;
import android.content.*;
import android.content.Context;
import android.provider.Settings;
import android.widget.*;
import android.graphics.Color;
import android.text.SpannableStringBuilder;
import android.text.Spanned;
import android.text.style.BackgroundColorSpan;
import android.text.style.UnderlineSpan;
import org.json.*;
import org.mozc.android.inputmethod.japanese.protobuf.ProtoCommands.*;
import org.mozc.android.inputmethod.japanese.protobuf.ProtoCandidateWindow.CandidateWindow;
import org.mozc.android.inputmethod.japanese.protobuf.ProtoCandidateWindow.CandidateList;
import org.mozc.android.inputmethod.japanese.protobuf.ProtoCandidateWindow.CandidateWord;
import java.io.IOException;
import java.util.ArrayDeque;

public final class KnitIme extends InputMethodService {
    static volatile KnitIme current;
    private volatile JapaneseEngine engine;
    private CompositionMode mode=CompositionMode.HIRAGANA;
    private android.content.ClipboardManager.OnPrimaryClipChangedListener clipboardListener;
    private LinearLayout candidates;
    private HorizontalScrollView candidateScroll;
    private TextView help;
    private Button modeButton;
    private Output lastOutput;
    private int candidatePage;
    private String preedit="",error="";
    private int selection,composingStart,expectedSelection=-1;
    private boolean destroyed,updating;
    private final ArrayDeque<JSONObject> pending=new ArrayDeque<>();
    static boolean selected(Context c) { return new ComponentName(c,KnitIme.class).flattenToShortString().equals(Settings.Secure.getString(c.getContentResolver(),Settings.Secure.DEFAULT_INPUT_METHOD)); }
    static boolean japaneseReady() {KnitIme ime=current;return ime!=null&&ime.engine!=null;}
    @Override public void onCreate() {
        super.onCreate();current=this;
        clipboardListener=()->{if(selected(this))Sharing.sendClipboard(this);};
        ((android.content.ClipboardManager)getSystemService(CLIPBOARD_SERVICE)).addPrimaryClipChangedListener(clipboardListener);
        JapaneseEngine.prepare(this,(ready,failure)->{
            if(destroyed){if(ready!=null)ready.close();return;}
            if(failure!=null){error=failure.getMessage();pending.clear();render(null);return;}
            engine=ready;
            try{engine.reset();engine.mode(mode);}catch(IOException e){fail();}
            render(null);
            if(ConnectionService.current!=null)ConnectionService.current.permissions();
            while(engine!=null&&!pending.isEmpty())receive(pending.removeFirst());
        });
    }
    private int dp(int n){return Math.round(n*getResources().getDisplayMetrics().density);}
    @Override public View onCreateInputView() {
        LinearLayout layout=new LinearLayout(this);layout.setOrientation(LinearLayout.VERTICAL);layout.setPadding(dp(12),dp(6),dp(12),dp(6));layout.setBackgroundColor(Color.rgb(242,245,250));
        help=new TextView(this);help.setTextSize(13);help.setTextColor(Color.rgb(77,89,113));layout.addView(help);
        HorizontalScrollView scroll=new HorizontalScrollView(this);candidateScroll=scroll;scroll.setHorizontalScrollBarEnabled(false);
        candidates=new LinearLayout(this);scroll.addView(candidates);layout.addView(scroll,new LinearLayout.LayoutParams(-1,dp(48)));
        LinearLayout row=new LinearLayout(this);layout.addView(row);
        modeButton=new Button(this);modeButton.setFocusable(false);modeButton.setOnClickListener(v->changeMode(mode==CompositionMode.HIRAGANA?CompositionMode.HALF_ASCII:mode==CompositionMode.HALF_ASCII?CompositionMode.FULL_ASCII:CompositionMode.HIRAGANA));row.addView(modeButton,new LinearLayout.LayoutParams(dp(64),dp(44)));
        Button paste=new Button(this);paste.setFocusable(false);paste.setText(app.knit.R.string.ime_paste);paste.setOnClickListener(v->paste());row.addView(paste,new LinearLayout.LayoutParams(0,dp(44),1));
        Button change=new Button(this);change.setFocusable(false);change.setText(app.knit.R.string.ime_switch);change.setOnClickListener(v->((InputMethodManager)getSystemService(INPUT_METHOD_SERVICE)).showInputMethodPicker());row.addView(change,new LinearLayout.LayoutParams(0,dp(44),1));
        render(lastOutput);return layout;
    }
    @Override public boolean onEvaluateFullscreenMode(){return false;}
    // Remote hardware input still needs its candidate row on the tablet.
    @Override public boolean onEvaluateInputViewShown(){super.onEvaluateInputViewShown();return true;}
    private void render(Output output) {
        lastOutput=output;
        if(help==null)return;
        help.setText(!error.isEmpty()?error:engine==null?"日本語入力を準備しています…":"Macから入力 · Spaceで変換 · Enterで確定");
        modeButton.setText(mode==CompositionMode.HALF_ASCII?"A":mode==CompositionMode.FULL_ASCII?"Ａ":"あ");
        modeButton.setContentDescription(mode==CompositionMode.HALF_ASCII?"英数入力。タップで全角英数へ":mode==CompositionMode.FULL_ASCII?"全角英数入力。タップで日本語へ":"日本語入力。タップで英数へ");
        candidatePage=output!=null&&output.hasAllCandidateWords()?output.getAllCandidateWords().getFocusedIndex()/9*9:0;
        renderCandidates();
    }
    private void renderCandidates() {
        candidates.removeAllViews();
        Output output=lastOutput;if(output==null)return;
        if(output.hasAllCandidateWords()) {
            CandidateList list=output.getAllCandidateWords();int count=list.getCandidatesCount();
            if(candidatePage>0)addCandidatePage("‹",Math.max(0,candidatePage-9));
            for(int i=candidatePage;i<Math.min(count,candidatePage+9);i++) {
                CandidateWord word=list.getCandidates(i);
                addCandidate(word.getValue(),word.getId(),word.hasId(),list.hasFocusedIndex()&&i==list.getFocusedIndex());
            }
            if(candidatePage+9<count)addCandidatePage("›",candidatePage+9);
            return;
        }
        if(!output.hasCandidateWindow())return;
        CandidateWindow window=output.getCandidateWindow();
        for(CandidateWindow.Candidate candidate:window.getCandidateList()) {
            addCandidate(candidate.getValue(),candidate.getId(),candidate.hasId(),window.hasFocusedIndex()&&candidate.getIndex()==window.getFocusedIndex());
        }
    }
    private void addCandidatePage(String title,int page) {
        Button button=new Button(this);button.setFocusable(false);button.setText(title);button.setContentDescription(title.equals("‹")?"前の変換候補":"次の変換候補");
        button.setOnClickListener(v->{candidatePage=page;renderCandidates();});candidates.addView(button,new LinearLayout.LayoutParams(dp(48),dp(48)));
    }
    private void addCandidate(String value,int id,boolean selectable,boolean focused) {
            Button button=new Button(this);button.setFocusable(false);button.setText(value);button.setTextSize(17);button.setAllCaps(false);
            button.setContentDescription("変換候補 "+value);
            if(focused){button.setTextColor(Color.rgb(47,80,182));candidateScroll.post(()->{if(button.getParent()==candidates)candidateScroll.smoothScrollTo(Math.max(0,button.getLeft()-dp(12)),0);});}
            if(selectable)button.setOnClickListener(v->{
                if(engine==null||getCurrentInputConnection()==null)return;
                try{apply(engine.command(SessionCommand.newBuilder().setType(SessionCommand.CommandType.SELECT_CANDIDATE).setId(id).build()));}catch(IOException e){fail();}
            });
            candidates.addView(button,new LinearLayout.LayoutParams(-2,dp(48)));
    }
    private void apply(Output output) {
        if(output==null)return;
        InputConnection input=getCurrentInputConnection();if(input==null)return;
        updating=true;input.beginBatchEdit();
        try {
            boolean committed=output.hasResult()&&!output.getResult().getValue().isEmpty();
            if(committed) {
                String text=output.getResult().getValue();int start=preedit.isEmpty()?selection:composingStart;input.commitText(text,1);
                int offset=output.getResult().getCursorOffset();
                int relative=Math.max(0,Math.min(text.codePointCount(0,text.length()),text.codePointCount(0,text.length())+offset));
                selection=start+JapaneseKeys.utf16Cursor(text,relative);
                if(offset!=0)input.setSelection(selection,selection);
                preedit="";composingStart=selection;
            }
            SpannableStringBuilder next=new SpannableStringBuilder();
            if(output.hasPreedit())for(Preedit.Segment segment:output.getPreedit().getSegmentList()) {
                int start=next.length();next.append(segment.getValue());
                if(next.length()>start) {
                    next.setSpan(new UnderlineSpan(),start,next.length(),Spanned.SPAN_EXCLUSIVE_EXCLUSIVE);
                    if(segment.getAnnotation()==Preedit.Segment.Annotation.HIGHLIGHT)next.setSpan(new BackgroundColorSpan(0xffdce5ff),start,next.length(),Spanned.SPAN_EXCLUSIVE_EXCLUSIVE);
                }
            }
            if(next.length()>0) {
                if(preedit.isEmpty())composingStart=selection;
                preedit=next.toString();input.setComposingText(next,1);
                int cursor=JapaneseKeys.utf16Cursor(preedit,output.getPreedit().getCursor());
                expectedSelection=composingStart+cursor;selection=expectedSelection;
                if(composingStart>=0)input.setSelection(expectedSelection,expectedSelection);
            } else {
                if(!preedit.isEmpty()&&!committed)input.setComposingText("",1);
                input.finishComposingText();preedit="";expectedSelection=selection;
            }
        } finally{input.endBatchEdit();updating=false;}
        render(output);
    }
    private void changeMode(CompositionMode next) {
        mode=next;
        if(engine!=null)try{apply(engine.mode(mode));}catch(IOException e){fail();}
        render(lastOutput);
    }
    private void fail() {
        JapaneseEngine old=engine;engine=null;if(old!=null)old.close();pending.clear();
        error="日本語変換を開始できません。Knitキーボードを選び直してください。";
        InputConnection input=getCurrentInputConnection();if(input!=null)input.finishComposingText();preedit="";render(null);
        if(ConnectionService.current!=null)ConnectionService.current.permissions();ConnectionService.report(error);
    }
    void receive(JSONObject message) {
        if(!selected(this))return;
        InputConnection input=getCurrentInputConnection();String t=message.optString("t");
        if(t.equals("ime")) {changeMode(message.optBoolean("kana")?CompositionMode.HIRAGANA:CompositionMode.HALF_ASCII);return;}
        if(input==null) {if(!t.equals("key")||message.optBoolean("down"))ConnectionService.report("入力する欄を先にタップしてください。");return;}
        if(t.equals("text")) {String text=message.optString("text");if(text.length()<=1024*1024){release();input.commitText(text,1);}return;}
        if(!message.optBoolean("down"))return;
        int kc=message.optInt("kc");
        if(kc==102){changeMode(CompositionMode.HALF_ASCII);return;}if(kc==104){changeMode(CompositionMode.HIRAGANA);return;}
        boolean command=message.optBoolean("cmd")||message.optBoolean("ctrl")||message.optBoolean("rcmd");
        if(engine==null&&error.isEmpty()) {if(pending.size()<128)pending.addLast(message);return;}
        if(engine!=null)try {
            Output output=engine.key(kc,message.optBoolean("shift"),command,message.optBoolean("opt"),mode);
            if(output!=null){apply(output);if(output.getConsumed())return;}
        }catch(IOException e){fail();return;}
        if(command) {
            int action=kc==0?android.R.id.selectAll:kc==8?android.R.id.copy:kc==7?android.R.id.cut:kc==9?android.R.id.paste:0;
            if(action!=0) {release();if(action==android.R.id.paste)paste();else input.performContextMenuAction(action);return;}
        }
        int androidKey=KeyTranslator.androidKey(kc);
        if(androidKey!=0) {
            int meta=(command?KeyEvent.META_CTRL_ON:0)|(message.optBoolean("shift")?KeyEvent.META_SHIFT_ON:0)|(message.optBoolean("opt")?KeyEvent.META_ALT_ON:0);
            long now=android.os.SystemClock.uptimeMillis();
            input.sendKeyEvent(new KeyEvent(now,now,KeyEvent.ACTION_DOWN,androidKey,0,meta));input.sendKeyEvent(new KeyEvent(now,now,KeyEvent.ACTION_UP,androidKey,0,meta));return;
        }
        if(command||message.optBoolean("opt"))return;
        if(mode!=CompositionMode.HALF_ASCII){ConnectionService.report(error.isEmpty()?"日本語入力を準備しています。":error);return;}
        String text=KeyTranslator.character(kc,message.optBoolean("shift"));if(text!=null)input.commitText(text,1);
    }
    void paste() {InputConnection input=getCurrentInputConnection();if(input==null)return;release();Sharing.applyPending(this);input.performContextMenuAction(android.R.id.paste);}
    void release() {
        InputConnection input=getCurrentInputConnection();if(input!=null)input.finishComposingText();
        if(engine!=null)try{engine.reset();}catch(IOException e){fail();}
        preedit="";pending.clear();render(null);
    }
    @Override public void onStartInput(EditorInfo info,boolean restarting) {
        super.onStartInput(info,restarting);pending.clear();preedit="";selection=Math.max(0,Math.min(info.initialSelStart,info.initialSelEnd));composingStart=selection;expectedSelection=-1;
        if(engine!=null)try{engine.reset();engine.mode(mode);}catch(IOException e){fail();}
        render(null);Sharing.applyPending(this);if(ConnectionService.current!=null)ConnectionService.current.permissions();
    }
    @Override public void onUpdateSelection(int oldStart,int oldEnd,int start,int end,int candidateStart,int candidateEnd) {
        super.onUpdateSelection(oldStart,oldEnd,start,end,candidateStart,candidateEnd);selection=Math.max(0,start);
        if(!updating&&!preedit.isEmpty()&&(start!=expectedSelection||end!=expectedSelection)) {
            if(candidateStart<0||start!=end||start<candidateStart||start>candidateEnd)release();
            else if(engine!=null)try {
                composingStart=candidateStart;int relative=Math.min(preedit.length(),Math.max(0,start-candidateStart));
                int cursor=preedit.codePointCount(0,relative);
                apply(engine.command(SessionCommand.newBuilder().setType(SessionCommand.CommandType.MOVE_CURSOR).setCursorPosition(cursor).build()));
            }catch(IOException e){fail();}
        }
    }
    @Override public void onFinishInput(){release();super.onFinishInput();}
    @Override public void onDestroy(){destroyed=true;pending.clear();JapaneseEngine old=engine;engine=null;if(old!=null)old.close();((android.content.ClipboardManager)getSystemService(CLIPBOARD_SERVICE)).removePrimaryClipChangedListener(clipboardListener);if(current==this)current=null;super.onDestroy();}
}
