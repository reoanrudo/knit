package app.knit;
import org.junit.Test;
import static org.junit.Assert.*;
import org.mozc.android.inputmethod.japanese.protobuf.ProtoCommands.KeyEvent;

public class JapaneseKeysTest {
    @Test public void hardwareConversionKeysReachMozcInsteadOfTheEditor() {
        assertEquals(KeyEvent.SpecialKey.SPACE,JapaneseKeys.event(49,false,false,false,true).getSpecialKey());
        assertEquals(KeyEvent.SpecialKey.ENTER,JapaneseKeys.event(36,false,false,false,true).getSpecialKey());
        assertEquals(KeyEvent.SpecialKey.F9,JapaneseKeys.event(101,false,false,false,true).getSpecialKey());
        assertEquals(KeyEvent.SpecialKey.BACKSPACE,JapaneseKeys.event(51,false,false,false,true).getSpecialKey());
    }
    @Test public void printableKeysAndShortcutModifiersAreDistinct() {
        KeyEvent printable=JapaneseKeys.event(0,true,false,false,true);
        assertEquals('A',printable.getKeyCode());assertEquals(0,printable.getModifierKeysCount());assertTrue(printable.getActivated());
        KeyEvent shortcut=JapaneseKeys.event(8,false,true,false,true);
        assertEquals('c',shortcut.getKeyCode());assertEquals(KeyEvent.ModifierKey.CTRL,shortcut.getModifierKeys(0));
        assertNull(JapaneseKeys.event(55,false,false,false,true));
    }
    @Test public void utf16CursorDoesNotSplitEmojiOrLoseJapaneseCharacters() {
        assertEquals(0,JapaneseKeys.utf16Cursor("日😀本",0));
        assertEquals(1,JapaneseKeys.utf16Cursor("日😀本",1));
        assertEquals(3,JapaneseKeys.utf16Cursor("日😀本",2));
        assertEquals(4,JapaneseKeys.utf16Cursor("日😀本",99));
    }
}
