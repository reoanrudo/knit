package app.knit;
import org.junit.Test;
import static org.junit.Assert.*;
public class KeyTranslatorTest {
    @Test public void shortcutsDoNotBecomeText() {
        assertNull(KeyTranslator.character(51,false));
        assertEquals(67,KeyTranslator.androidKey(51));
        assertNull(KeyTranslator.character(123,true));
        assertEquals(21,KeyTranslator.androidKey(123));
        assertNull(KeyTranslator.character(104,false));
        assertEquals(0,KeyTranslator.androidKey(104));
    }
    @Test public void physicalKeysIncludeCaseAndSymbols() {
        assertEquals("a",KeyTranslator.character(0,false));assertEquals("A",KeyTranslator.character(0,true));
        assertEquals("!",KeyTranslator.character(18,true));assertEquals("0",KeyTranslator.character(29,false));
        assertEquals("?",KeyTranslator.character(44,true));assertEquals("\\",KeyTranslator.character(93,false));
    }
}
