use pom::parser::*;

use crate::cmap_section::{ArrayOfTargetStrings, CMapParseError, CMapSection, SourceCharMapping, SourceRangeMapping};
use crate::parser::{dictionary, eol, hex_char, name, space};

fn hex_u16<'a>() -> Parser<'a, u8, u16> {
    (hex_char() + hex_char()).map(|(u1, u2)| u1 as u16 * 256 + u2 as u16)
}

fn space_no_crlf<'a>() -> Parser<'a, u8, ()> {
    one_of(b" \t").repeat(0..).discard()
}

fn ws_newline<'a>() -> Parser<'a, u8, ()> {
    space_no_crlf() * eol().discard()
}

fn whitespace<'a>() -> Parser<'a, u8, ()> {
    one_of(b" \t\n\r\0\x0C").repeat(1..).discard()
}

fn source_code<'a>() -> Parser<'a, u8, u16> {
    sym(b'<') * hex_u16() - sym(b'>')
}

fn code_range_pair<'a>() -> Parser<'a, u8, (u16, u16)> {
    space_no_crlf() * source_code() + space_no_crlf() * source_code()
}

fn codespace_range_section<'a>() -> Parser<'a, u8, Vec<(u16, u16)>> {
    one_of(b"0123456789").repeat(1..)
        * space_no_crlf()
        * seq(b"begincodespacerange")
        * ws_newline()
        * (code_range_pair() - ws_newline()).repeat(1..)
        - seq(b"endcodespacerange")
        - ws_newline()
}

fn target_string<'a>() -> Parser<'a, u8, Vec<u16>> {
    // no more than 512 bytes in a target string
    sym(b'<') * hex_u16().repeat(1..256) - sym(b'>')
}

fn range_target_array<'a>() -> Parser<'a, u8, ArrayOfTargetStrings> {
    sym(b'[') * space_no_crlf() * (target_string() - space_no_crlf()).repeat(1..) - sym(b']')
}

fn bf_range_line<'a>() -> Parser<'a, u8, SourceRangeMapping> {
    code_range_pair() + space_no_crlf() * (target_string().map(|it| vec![it]) | range_target_array()) - ws_newline()
}

fn bf_range_section<'a>() -> Parser<'a, u8, Vec<SourceRangeMapping>> {
    one_of(b"0123456789").repeat(1..)
        * space_no_crlf()
        * seq(b"beginbfrange")
        * ws_newline()
        * bf_range_line().repeat(1..)
        - seq(b"endbfrange")
        - ws_newline()
}

fn bf_char_line<'a>() -> Parser<'a, u8, SourceCharMapping> {
    space_no_crlf() * source_code() + space_no_crlf() * target_string() - space_no_crlf() - eol().discard()
}

fn bf_char_section<'a>() -> Parser<'a, u8, Vec<SourceCharMapping>> {
    one_of(b"0123456789").repeat(1..)
        * space_no_crlf()
        * seq(b"beginbfchar")
        * ws_newline()
        * bf_char_line().repeat(1..)
        - seq(b"endbfchar")
        - ws_newline()
}

fn cmap_type<'a>() -> Parser<'a, u8, ()> {
    space_no_crlf() * seq(b"/CMapType") * space_no_crlf() * sym(b'2') * space_no_crlf() * seq(b"def") * ws_newline()
}

fn cmap_name<'a>() -> Parser<'a, u8, ()> {
    space_no_crlf() * seq(b"/CMapName") * space_no_crlf() * name() * space_no_crlf() * seq(b"def") * ws_newline()
}

fn cid_system_info<'a>() -> Parser<'a, u8, ()> {
    space_no_crlf() * seq(b"/CIDSystemInfo") * space() * dictionary() * space() * seq(b"def") * ws_newline()
}

fn cmap_stream<'a>() -> Parser<'a, u8, Vec<CMapSection>> {
    use self::space_no_crlf as ws;
    use self::ws_newline as nl;
    ws() * seq(b"/CIDInit")
        * ws()
        * seq(b"/ProcSet")
        * ws()
        * seq(b"findresource")
        * ws()
        * seq(b"begin")
        * nl()
        * ws()
        * one_of(b"0123456789").repeat(1..)
        * ws()
        * seq(b"dict")
        * ws()
        * seq(b"begin")
        * nl()
        * ws()
        * seq(b"begincmap")
        * nl()
        * (cmap_type() | cmap_name() | cid_system_info()).repeat(1..4)
        * (codespace_range_section().map(CMapSection::CsRange)
            | bf_char_section().map(CMapSection::BfChar)
            | bf_range_section().map(CMapSection::BfRange))
        .repeat(1..)
        - ws() * seq(b"endcmap") * nl()
        - ws()
            * seq(b"CMapName")
            * ws()
            * seq(b"currentdict")
            * ws()
            * seq(b"/CMap")
            * ws()
            * seq(b"defineresource")
            * ws()
            * seq(b"pop")
            * nl()
            * ws()
            * seq(b"end")
            * whitespace()
            * seq(b"end")
}

pub(crate) fn parse(stream_content: &[u8]) -> Result<Vec<CMapSection>, CMapParseError> {
    cmap_stream().parse(stream_content).map_err(CMapParseError::from)
}

impl From<pom::Error> for CMapParseError {
    fn from(err: pom::Error) -> Self {
        match err {
            pom::Error::Incomplete => CMapParseError::Incomplete,
            _ => CMapParseError::Error,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_source_code() {
        let data = "<080F>";
        assert_eq!(source_code().parse(data.as_bytes()), Ok(0x080f))
    }

    #[test]
    fn parse_invalid_source_code() {
        let data = "<080f01>";
        assert!(source_code().parse(data.as_bytes()).is_err())
    }

    #[test]
    fn parse_code_range_pair() {
        let data = "<080F> <08FF> ";
        assert_eq!(code_range_pair().parse(data.as_bytes()), Ok((0x080f, 0x08ff)),)
    }

    #[test]
    fn parse_bfrange_line() {
        let data = "<080f> <08ff> <09000110>\n";
        assert_eq!(
            bf_range_line().parse(data.as_bytes()),
            Ok(((0x080f, 0x08ff), vec![vec![0x0900, 0x0110]]))
        )
    }
    #[test]
    fn parse_bfrange_line_array() {
        let data = "<080f> <08ff> [ <09000110> <08fe> ] \n";
        assert_eq!(
            bf_range_line().parse(data.as_bytes()),
            Ok(((0x080f, 0x08ff), vec![vec![0x0900, 0x0110], vec![0x08fe]]))
        )
    }
    #[test]
    fn parse_invalid_bfrange_line() {
        let data = "<080f> <08ff> [ <09000110> <08FF> <09fe80> ]\n";
        assert!(bf_range_line().parse(data.as_bytes()).is_err())
    }

    #[test]
    fn parse_codespace_range_section() {
        let data = "1 begincodespacerange\n\
            <0000> <FFFF> \n\
        endcodespacerange\n";
        assert_eq!(
            codespace_range_section().parse(data.as_bytes()),
            Ok(vec![(0x0000, 0xffff)])
        )
    }

    #[test]
    fn parse_bf_range_section() {
        let data = "3 beginbfrange \n\
            <0000> <000f> <0000>\n\
            <0010> <001f> <00000010> \n\
            <0020>  <002f> [<0000> <00000010> ]\n\
        endbfrange\n";
        assert_eq!(
            bf_range_section().parse(data.as_bytes()),
            Ok(vec![
                ((0x0000, 0x000f), vec![vec![0x0000]]),
                ((0x0010, 0x001f), vec![vec![0x0000, 0x0010]]),
                ((0x0020, 0x002f), vec![vec![0x0000], vec![0x0000, 0x0010]]),
            ])
        )
    }

    #[test]
    fn parse_cid_system_info() {
        let data = "/CIDSystemInfo <<
/Registry (Adobe)
/Ordering (UCS)
/Supplement 0
>> def
";
        assert!(cid_system_info().parse(data.as_bytes()).is_ok())
    }

    #[test]
    fn parse_cid_system_info_with_spaces() {
        let data = "/CIDSystemInfo
<< /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def\n";
        assert!(cid_system_info().parse(data.as_bytes()).is_ok())
    }

    #[test]
    fn parse_cmap_name() {
        let data = "/CMapName /Adobe-Identity-UCS def\n";
        assert!(cmap_name().parse(data.as_bytes()).is_ok())
    }

    #[test]
    fn parse_cmap_name2() {
        let data = "/CMapName /Adobe-UCS-0 def\n";
        assert!(cmap_name().parse(data.as_bytes()).is_ok())
    }

    #[test]
    fn parse_cmap_type() {
        let data = "/CMapType 2 def\n";
        assert!(cmap_type().parse(data.as_bytes()).is_ok())
    }

    #[test]
    fn parse_cmap_section_1() {
        let data = "/CIDInit /ProcSet findresource begin
12 dict begin
begincmap
/CIDSystemInfo <<
/Registry (Adobe)
/Ordering (UCS)
/Supplement 0
>> def
/CMapName /Adobe-Identity-UCS def
/CMapType 2 def
1 begincodespacerange
<0000> <FFFF>
endcodespacerange
96 beginbfrange
<0000> <00FF> <0000>
<0100> <01FF> <0100>
<0200> <02FF> <0200>
<0300> <03FF> <0300>
<0400> <04FF> <0400>
<0500> <05FF> <0500>
<0600> <06FF> <0600>
<0700> <07FF> <0700>
<0800> <08FF> <0800>
<0900> <09FF> <0900>
<0A00> <0AFF> <0A00>
<0B00> <0BFF> <0B00>
<0C00> <0CFF> <0C00>
<0D00> <0DFF> <0D00>
<0E00> <0EFF> <0E00>
<0F00> <0FFF> <0F00>
<1000> <10FF> <1000>
<1100> <11FF> <1100>
<1200> <12FF> <1200>
<1300> <13FF> <1300>
<1400> <14FF> <1400>
<1500> <15FF> <1500>
<1600> <16FF> <1600>
<1700> <17FF> <1700>
<1800> <18FF> <1800>
<1900> <19FF> <1900>
<1A00> <1AFF> <1A00>
<1B00> <1BFF> <1B00>
<1C00> <1CFF> <1C00>
<1D00> <1DFF> <1D00>
<1E00> <1EFF> <1E00>
<1F00> <1FFF> <1F00>
<2000> <20FF> <2000>
<2100> <21FF> <2100>
<2200> <22FF> <2200>
<2300> <23FF> <2300>
<2400> <24FF> <2400>
<2500> <25FF> <2500>
<2600> <26FF> <2600>
<2700> <27FF> <2700>
<2800> <28FF> <2800>
<2900> <29FF> <2900>
<2A00> <2AFF> <2A00>
<2B00> <2BFF> <2B00>
<2C00> <2CFF> <2C00>
<2D00> <2DFF> <2D00>
<2E00> <2EFF> <2E00>
<2F00> <2FFF> <2F00>
<3000> <30FF> <3000>
<3100> <31FF> <3100>
<3200> <32FF> <3200>
<3300> <33FF> <3300>
<3400> <34FF> <3400>
<3500> <35FF> <3500>
<3600> <36FF> <3600>
<3700> <37FF> <3700>
<3800> <38FF> <3800>
<3900> <39FF> <3900>
<3A00> <3AFF> <3A00>
<3B00> <3BFF> <3B00>
<3C00> <3CFF> <3C00>
<3D00> <3DFF> <3D00>
<3E00> <3EFF> <3E00>
<3F00> <3FFF> <3F00>
<4000> <40FF> <4000>
<4100> <41FF> <4100>
<4200> <42FF> <4200>
<4300> <43FF> <4300>
<4400> <44FF> <4400>
<4500> <45FF> <4500>
<4600> <46FF> <4600>
<4700> <47FF> <4700>
<4800> <48FF> <4800>
<4900> <49FF> <4900>
<4A00> <4AFF> <4A00>
<4B00> <4BFF> <4B00>
<4C00> <4CFF> <4C00>
<4D00> <4DFF> <4D00>
<4E00> <4EFF> <4E00>
<4F00> <4FFF> <4F00>
<5000> <50FF> <5000>
<5100> <51FF> <5100>
<5200> <52FF> <5200>
<5300> <53FF> <5300>
<5400> <54FF> <5400>
<5500> <55FF> <5500>
<5600> <56FF> <5600>
<5700> <57FF> <5700>
<5800> <58FF> <5800>
<5900> <59FF> <5900>
<5A00> <5AFF> <5A00>
<5B00> <5BFF> <5B00>
<5C00> <5CFF> <5C00>
<5D00> <5DFF> <5D00>
<5E00> <5EFF> <5E00>
<5F00> <5FFF> <5F00>
endbfrange
96 beginbfrange
<6000> <60FF> <6000>
<6100> <61FF> <6100>
<6200> <62FF> <6200>
<6300> <63FF> <6300>
<6400> <64FF> <6400>
<6500> <65FF> <6500>
<6600> <66FF> <6600>
<6700> <67FF> <6700>
<6800> <68FF> <6800>
<6900> <69FF> <6900>
<6A00> <6AFF> <6A00>
<6B00> <6BFF> <6B00>
<6C00> <6CFF> <6C00>
<6D00> <6DFF> <6D00>
<6E00> <6EFF> <6E00>
<6F00> <6FFF> <6F00>
<7000> <70FF> <7000>
<7100> <71FF> <7100>
<7200> <72FF> <7200>
<7300> <73FF> <7300>
<7400> <74FF> <7400>
<7500> <75FF> <7500>
<7600> <76FF> <7600>
<7700> <77FF> <7700>
<7800> <78FF> <7800>
<7900> <79FF> <7900>
<7A00> <7AFF> <7A00>
<7B00> <7BFF> <7B00>
<7C00> <7CFF> <7C00>
<7D00> <7DFF> <7D00>
<7E00> <7EFF> <7E00>
<7F00> <7FFF> <7F00>
<8000> <80FF> <8000>
<8100> <81FF> <8100>
<8200> <82FF> <8200>
<8300> <83FF> <8300>
<8400> <84FF> <8400>
<8500> <85FF> <8500>
<8600> <86FF> <8600>
<8700> <87FF> <8700>
<8800> <88FF> <8800>
<8900> <89FF> <8900>
<8A00> <8AFF> <8A00>
<8B00> <8BFF> <8B00>
<8C00> <8CFF> <8C00>
<8D00> <8DFF> <8D00>
<8E00> <8EFF> <8E00>
<8F00> <8FFF> <8F00>
<9000> <90FF> <9000>
<9100> <91FF> <9100>
<9200> <92FF> <9200>
<9300> <93FF> <9300>
<9400> <94FF> <9400>
<9500> <95FF> <9500>
<9600> <96FF> <9600>
<9700> <97FF> <9700>
<9800> <98FF> <9800>
<9900> <99FF> <9900>
<9A00> <9AFF> <9A00>
<9B00> <9BFF> <9B00>
<9C00> <9CFF> <9C00>
<9D00> <9DFF> <9D00>
<9E00> <9EFF> <9E00>
<9F00> <9FFF> <9F00>
<A000> <A0FF> <A000>
<A100> <A1FF> <A100>
<A200> <A2FF> <A200>
<A300> <A3FF> <A300>
<A400> <A4FF> <A400>
<A500> <A5FF> <A500>
<A600> <A6FF> <A600>
<A700> <A7FF> <A700>
<A800> <A8FF> <A800>
<A900> <A9FF> <A900>
<AA00> <AAFF> <AA00>
<AB00> <ABFF> <AB00>
<AC00> <ACFF> <AC00>
<AD00> <ADFF> <AD00>
<AE00> <AEFF> <AE00>
<AF00> <AFFF> <AF00>
<B000> <B0FF> <B000>
<B100> <B1FF> <B100>
<B200> <B2FF> <B200>
<B300> <B3FF> <B300>
<B400> <B4FF> <B400>
<B500> <B5FF> <B500>
<B600> <B6FF> <B600>
<B700> <B7FF> <B700>
<B800> <B8FF> <B800>
<B900> <B9FF> <B900>
<BA00> <BAFF> <BA00>
<BB00> <BBFF> <BB00>
<BC00> <BCFF> <BC00>
<BD00> <BDFF> <BD00>
<BE00> <BEFF> <BE00>
<BF00> <BFFF> <BF00>
endbfrange
64 beginbfrange
<C000> <C0FF> <C000>
<C100> <C1FF> <C100>
<C200> <C2FF> <C200>
<C300> <C3FF> <C300>
<C400> <C4FF> <C400>
<C500> <C5FF> <C500>
<C600> <C6FF> <C600>
<C700> <C7FF> <C700>
<C800> <C8FF> <C800>
<C900> <C9FF> <C900>
<CA00> <CAFF> <CA00>
<CB00> <CBFF> <CB00>
<CC00> <CCFF> <CC00>
<CD00> <CDFF> <CD00>
<CE00> <CEFF> <CE00>
<CF00> <CFFF> <CF00>
<D000> <D0FF> <D000>
<D100> <D1FF> <D100>
<D200> <D2FF> <D200>
<D300> <D3FF> <D300>
<D400> <D4FF> <D400>
<D500> <D5FF> <D500>
<D600> <D6FF> <D600>
<D700> <D7FF> <D700>
<D800> <D8FF> <D800>
<D900> <D9FF> <D900>
<DA00> <DAFF> <DA00>
<DB00> <DBFF> <DB00>
<DC00> <DCFF> <DC00>
<DD00> <DDFF> <DD00>
<DE00> <DEFF> <DE00>
<DF00> <DFFF> <DF00>
<E000> <E0FF> <E000>
<E100> <E1FF> <E100>
<E200> <E2FF> <E200>
<E300> <E3FF> <E300>
<E400> <E4FF> <E400>
<E500> <E5FF> <E500>
<E600> <E6FF> <E600>
<E700> <E7FF> <E700>
<E800> <E8FF> <E800>
<E900> <E9FF> <E900>
<EA00> <EAFF> <EA00>
<EB00> <EBFF> <EB00>
<EC00> <ECFF> <EC00>
<ED00> <EDFF> <ED00>
<EE00> <EEFF> <EE00>
<EF00> <EFFF> <EF00>
<F000> <F0FF> <F000>
<F100> <F1FF> <F100>
<F200> <F2FF> <F200>
<F300> <F3FF> <F300>
<F400> <F4FF> <F400>
<F500> <F5FF> <F500>
<F600> <F6FF> <F600>
<F700> <F7FF> <F700>
<F800> <F8FF> <F800>
<F900> <F9FF> <F900>
<FA00> <FAFF> <FA00>
<FB00> <FBFF> <FB00>\r
<FC00> <FCFF> <FC00>
<FD00> <FDFF> <FD00>
<FE00> <FEFF> <FE00>
<FF00> <FFFF> <FF00>
endbfrange
endcmap
CMapName currentdict /CMap defineresource pop
end
end";
        assert!(cmap_stream().parse(data.as_bytes()).is_ok())
    }

    #[test]
    fn parse_cmap_section_2() {
        let data = "/CIDInit /ProcSet findresource begin
12 dict begin
begincmap
/CMapType 2 def
/CMapName/R27 def
1 begincodespacerange
<0000><ffff>
endcodespacerange
78 beginbfrange
<0020><0020><0020>
<0028><0028><0028>
<0029><0029><0029>
<002b><002b><002b>
<002c><002c><002c>
<002d><002d><002d>
<002e><002e><002e>
<002f><002f><002f>
<0030><0030><0030>
<0031><0031><0031>
<0032><0032><0032>
<0033><0033><0033>
<0034><0034><0034>
<0035><0035><0035>
<0036><0036><0036>
<0037><0037><0037>
<0038><0038><0038>
<0039><0039><0039>
<003a><003a><003a>
<003d><003d><003d>
<0041><0041><0041>
<0042><0042><0042>
<0043><0043><0043>
<0044><0044><0044>
<0045><0045><0045>
<0046><0046><0046>
<0047><0047><0047>
<0048><0048><0048>
<0049><0049><0049>
<004a><004a><004a>
<004b><004b><004b>
<004c><004c><004c>
<004d><004d><004d>
<004e><004e><004e>
<004f><004f><004f>
<0050><0050><0050>
<0052><0052><0052>
<0053><0053><0053>
<0054><0054><0054>
<0055><0055><0055>
<0056><0056><0056>
<0057><0057><0057>
<0058><0058><0058>
<005a><005a><005a>
<005c><005c><005c>
<0061><0061><0061>
<0062><0062><0062>
<0063><0063><0063>
<0064><0064><0064>
<0065><0065><0065>
<0066><0066><0066>
<0067><0067><0067>
<0068><0068><0068>
<0069><0069><0069>
<006a><006a><006a>
<006b><006b><006b>
<006c><006c><006c>
<006d><006d><006d>
<006e><006e><006e>
<006f><006f><006f>
<0070><0070><0070>
<0072><0072><0072>
<0073><0073><0073>
<0074><0074><0074>
<0075><0075><0075>
<0076><0076><0076>
<0077><0077><0077>
<0078><0078><0078>
<0079><0079><0079>
<007a><007a><007a>
<007c><007c><007c>
<00a3><00a3><00a3>
<00d6><00d6><00d6>
<00df><00df><00df>
<00e4><00e4><00e4>
<00f6><00f6><00f6>
<00fc><00fc><00fc>
<2019><2019><2019>
endbfrange
endcmap
CMapName currentdict /CMap defineresource pop
end end
";
        assert!(cmap_stream().parse(data.as_bytes()).is_ok())
    }

    #[test]
    fn parse_cmap_section_bfchar() {
        let data = "/CIDInit /ProcSet findresource begin
12 dict begin
begincmap
/CIDSystemInfo
<< /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def
/CMapName /Adobe-UCS-0 def
/CMapType 2 def
1 begincodespacerange
<0000> <FFFF>
endcodespacerange
36 beginbfchar
<0000> <0000>
<0001> <004C>
<0002> <0069>
<0003> <0073>
<0004> <0074>
<0005> <0061>
<0006> <006F>
<0007> <0070>
<0008> <0065>
<0009> <0072>
<000A> <0063>
<000B> <006A>
<000C> <007A>
<000D> <006B>
<000E> <0064>
<000F> <0032>
<0010> <0030>
<0011> <0033>
<0012> <002D>
<0013> <0035>
<0014> <0031>
<0015> <0039>
<0016> <0034>
<0017> <0057>
<0018> <006C>
<0019> <0075>
<001A> <0142>
<001B> <0079>
<001C> <0077>
<001D> <004F>
<001E> <0044>
<001F> <0052>
<0020> <0068>
<0021> <006E>
<0022> <004B>
<0023> <0067>
endbfchar
endcmap
CMapName currentdict /CMap defineresource pop
end
end";
        assert!(cmap_stream().parse(data.as_bytes()).is_ok())
    }

    #[test]
    fn parse_cmap_section_with_discontigous_range() {
        let data = "/CIDInit /ProcSet findresource begin
12 dict begin
begincmap
/CIDSystemInfo
<< /Registry (Adobe)
/Ordering (UCS)
/Supplement 0
>> def
/CMapName /Adobe-Identity-UCS def
/CMapType 2 def
1 begincodespacerange
<0000> <FFFF>
endcodespacerange
2 beginbfrange
<0000> <005E> <0020>
<005F> <0061> [<D83DDE00> <D83DDD27> <D83DDD28>]
endbfrange
1 beginbfchar
<3A51> <D840DC3E>
endbfchar
endcmap
CMapName currentdict /CMap defineresource pop
end
end";
        assert!(cmap_stream().parse(data.as_bytes()).is_ok())
    }
}
