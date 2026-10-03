package app.knit;
/** Physical Mac keys: Unicode commits use the separate text message. */
final class KeyTranslator {
    static String character(int kc,boolean shift) {
        int[] codes={0,11,8,2,14,3,5,4,34,38,40,37,46,45,31,35,12,15,1,17,32,9,13,7,16,6};
        for(int i=0;i<codes.length;i++) if(codes[i]==kc) return String.valueOf((char)((shift?'A':'a')+i));
        int[] digits={29,18,19,20,21,23,22,26,28,25};
        for(int i=0;i<digits.length;i++) if(digits[i]==kc) return String.valueOf((shift?")!@#$%^&*(":"0123456789").charAt(i));
        switch(kc) {
            case 49:return " ";case 27:return shift?"_":"-";case 24:return shift?"+":"=";
            case 33:return shift?"{":"[";case 30:return shift?"}":"]";case 39:return shift?":":";";
            case 41:return shift?"\"":"'";case 42:case 93:return shift?"|":"\\";
            case 43:return shift?"<":",";case 47:return shift?">":".";case 44:return shift?"?":"/";
            case 50:return shift?"~":"`";default:return null;
        }
    }
    static int androidKey(int kc) {
        switch(kc) {
            case 36:case 76:return 66;case 48:return 61;case 51:return 67;case 117:return 112;
            case 53:return 111;case 123:return 21;case 124:return 22;case 125:return 20;case 126:return 19;
            case 115:return 122;case 119:return 123;case 116:return 92;case 121:return 93;default:return 0;
        }
    }
}
