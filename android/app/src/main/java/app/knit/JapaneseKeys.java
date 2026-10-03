package app.knit;

import org.mozc.android.inputmethod.japanese.protobuf.ProtoCommands.*;

/** Mac hardware keys to Mozc. No network or Android input state is read here. */
final class JapaneseKeys {
    private JapaneseKeys() {}
    static KeyEvent event(int kc, boolean shift, boolean control, boolean alt, boolean kana) {
        KeyEvent.SpecialKey special=switch(kc) {
            case 36,76->KeyEvent.SpecialKey.ENTER;case 48->KeyEvent.SpecialKey.TAB;
            case 49->KeyEvent.SpecialKey.SPACE;case 51->KeyEvent.SpecialKey.BACKSPACE;
            case 53->KeyEvent.SpecialKey.ESCAPE;case 117->KeyEvent.SpecialKey.DEL;
            case 123->KeyEvent.SpecialKey.LEFT;case 124->KeyEvent.SpecialKey.RIGHT;
            case 125->KeyEvent.SpecialKey.DOWN;case 126->KeyEvent.SpecialKey.UP;
            case 115->KeyEvent.SpecialKey.HOME;case 119->KeyEvent.SpecialKey.END;
            case 116->KeyEvent.SpecialKey.PAGE_UP;case 121->KeyEvent.SpecialKey.PAGE_DOWN;
            case 122->KeyEvent.SpecialKey.F1;case 120->KeyEvent.SpecialKey.F2;
            case 99->KeyEvent.SpecialKey.F3;case 118->KeyEvent.SpecialKey.F4;
            case 96->KeyEvent.SpecialKey.F5;case 97->KeyEvent.SpecialKey.F6;
            case 98->KeyEvent.SpecialKey.F7;case 100->KeyEvent.SpecialKey.F8;
            case 101->KeyEvent.SpecialKey.F9;case 109->KeyEvent.SpecialKey.F10;
            case 103->KeyEvent.SpecialKey.F11;case 111->KeyEvent.SpecialKey.F12;
            default->null;
        };
        KeyEvent.Builder b=KeyEvent.newBuilder().setActivated(kana).setMode(kana?CompositionMode.HIRAGANA:CompositionMode.HALF_ASCII);
        if(special!=null) b.setSpecialKey(special);
        else {
            String text=KeyTranslator.character(kc,shift);
            if(text==null || text.isEmpty())return null;
            b.setKeyCode(text.codePointAt(0));
        }
        if(control)b.addModifierKeys(KeyEvent.ModifierKey.CTRL);
        if(alt)b.addModifierKeys(KeyEvent.ModifierKey.ALT);
        if(shift&&(special!=null||control||alt))b.addModifierKeys(KeyEvent.ModifierKey.SHIFT);
        return b.build();
    }
    static int utf16Cursor(String text,int codePoints) {
        return text.offsetByCodePoints(0,Math.max(0,Math.min(codePoints,text.codePointCount(0,text.length()))));
    }
}
