//! Русский (Россия): the formula words Russian Excel shows (function names, booleans, errors,
//! structured-reference specifiers) and its formula separators.
//!
//! Function names come from Microsoft Support's ru-ru Excel function pages (retrieved
//! 2026-10-10; the pages are machine-translated): per page, a vote among the title, the meta
//! description, the syntax paragraph (aligned with the en-US page) and `=NAME(` examples, and an
//! entry needs two agreeing signals. Entries marked `reviewed` were decided by hand from the page
//! evidence and independent function-name references. Functions not listed keep their English name.
//!
//! Unless an entry says otherwise, `reviewed` means: At least two independent function translation references agree; conflicting machine-translated spellings rejected.
//!
//! Sources for the other words:
//! - bool: https://support.microsoft.com/ru-ru/excel/functions/address-function (a1 argument: ИСТИНА / ЛОЖЬ)
//! - errors: https://support.microsoft.com/ru-ru/excel/functions/error-type-function
//! - errors.#SPILL!: https://support.microsoft.com/ru-ru/excel/how-to-correct-a-spill-error
//! - table_items: https://support.microsoft.com/ru-ru/excel/using-structured-references-with-excel-tables
//! - errors.#CALC!: https://www.planetaexcel.ru/techniques/25/9457/ | https://coddy.tech/docs/ru/excel/filter (empty FILTER result -> #ВЫЧИСЛ!, English #CALC!)
//! - errors.#REF!: https://excel-academy.ru/faq/kak-ispravit-oshibki-ssyilka-i-imya-v-excel/ | https://coddy.tech/docs/ru/excel/ref-error | https://support.microsoft.com/ru-ru/excel/functions/sum-function (ERROR.TYPE table truncates spelling to #ССЫЛ!, corrected using independent formula examples)

use super::Dialect;
use gridcraft_core::CellError;

pub(super) const DIALECT: Dialect = Dialect {
    list: ';',
    decimal: ',',
    array_column: ';',
    array_row: ':',
    bool_true: "ИСТИНА",
    bool_false: "ЛОЖЬ",
    errors: &[
        (CellError::Null, "#ПУСТО!"),
        (CellError::Div0, "#ДЕЛ/0!"),
        (CellError::Value, "#ЗНАЧ!"),
        (CellError::Ref, "#ССЫЛКА!"),
        (CellError::Name, "#ИМЯ?"),
        (CellError::Num, "#ЧИСЛО!"),
        (CellError::NA, "#Н/Д"),
        (CellError::GettingData, "#ОЖИДАНИЕ_ДАННЫХ"),
        (CellError::Spill, "#ПЕРЕНОС!"),
        (CellError::Calc, "#ВЫЧИСЛ!"),
    ],
    table_items: ["#Все", "#Данные", "#Заголовки", "#Итого", "#Эта строка"],
    functions: &[
        ("ACCRINT", "НАКОПДОХОД"),
        ("ACCRINTM", "НАКОПДОХОДПОГАШ"),
        ("ADDRESS", "АДРЕС"),
        ("AGGREGATE", "АГРЕГАТ"),
        ("AMORDEGRC", "АМОРУМ"),
        ("AMORLINC", "АМОРУВ"),
        ("AND", "И"),
        ("ARABIC", "АРАБСКОЕ"),
        ("AREAS", "ОБЛАСТИ"),
        ("ARRAYTOTEXT", "МАССИВВТЕКСТ"),
        ("AVEDEV", "СРОТКЛ"),
        ("AVERAGE", "СРЗНАЧ"),
        ("AVERAGEA", "СРЗНАЧА"),
        ("AVERAGEIF", "СРЗНАЧЕСЛИ"),
        ("AVERAGEIFS", "СРЗНАЧЕСЛИМН"),
        // reviewed: perfectxl and excel-translator text table; MS page title and example =БАТТЕКСТ(A2) | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://en.excel-translator.de/functions/text/ | https://support.microsoft.com/ru-ru/excel/functions/bahttext-function
        ("BAHTTEXT", "БАТТЕКСТ"),
        ("BASE", "ОСНОВАНИЕ"),
        ("BESSELI", "БЕССЕЛЬ.I"),
        ("BESSELJ", "БЕССЕЛЬ.J"),
        ("BESSELK", "БЕССЕЛЬ.K"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/
        ("BESSELY", "БЕССЕЛЬ.Y"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/
        ("BETA.DIST", "БЕТА.РАСП"),
        ("BETA.INV", "БЕТА.ОБР"),
        ("BETADIST", "БЕТАРАСП"),
        ("BETAINV", "БЕТАОБР"),
        ("BIN2DEC", "ДВ.В.ДЕС"),
        ("BIN2HEX", "ДВ.В.ШЕСТН"),
        ("BIN2OCT", "ДВ.В.ВОСЬМ"),
        ("BINOM.DIST", "БИНОМ.РАСП"),
        ("BINOM.DIST.RANGE", "БИНОМ.РАСП.ДИАП"),
        ("BINOM.INV", "БИНОМ.ОБР"),
        ("BINOMDIST", "БИНОМРАСП"),
        ("BITAND", "БИТ.И"),
        ("BITLSHIFT", "БИТ.СДВИГЛ"),
        ("BITOR", "БИТ.ИЛИ"),
        ("BITRSHIFT", "БИТ.СДВИГП"),
        ("BITXOR", "БИТ.ИСКЛИЛИ"),
        ("CEILING", "ОКРВВЕРХ"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://easy-excel.com/excel-in-other-languages/excel-functions-in-russian/ | https://excel-academy.ru/tools/excel-functions/
        ("CEILING.MATH", "ОКРВВЕРХ.МАТ"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("CEILING.PRECISE", "ОКРВВЕРХ.ТОЧН"),
        ("CELL", "ЯЧЕЙКА"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://easy-excel.com/excel-in-other-languages/excel-functions-in-russian/ | https://excel-academy.ru/tools/excel-functions/
        ("CHAR", "СИМВОЛ"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/
        ("CHIDIST", "ХИ2РАСП"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/
        ("CHIINV", "ХИ2ОБР"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/
        ("CHISQ.DIST", "ХИ2.РАСП"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/
        ("CHISQ.DIST.RT", "ХИ2.РАСП.ПХ"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/
        ("CHISQ.INV", "ХИ2.ОБР"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/
        ("CHISQ.INV.RT", "ХИ2.ОБР.ПХ"),
        ("CHISQ.TEST", "ХИ2.ТЕСТ"),
        ("CHITEST", "ХИ2ТЕСТ"),
        ("CHOOSE", "ВЫБОР"),
        ("CHOOSECOLS", "ВЫБОРСТОЛБЦ"),
        ("CLEAN", "ПЕЧСИМВ"),
        ("CODE", "КОДСИМВ"),
        ("COLUMN", "СТОЛБЕЦ"),
        ("COLUMNS", "ЧИСЛСТОЛБ"),
        ("COMBIN", "ЧИСЛКОМБ"),
        ("COMBINA", "ЧИСЛКОМБА"),
        ("COMPLEX", "КОМПЛЕКСН"),
        // reviewed: Title and metadata agree; syntax/examples incorrectly copied from CONCATENATE; common function table corroborates СЦЕП | sources: https://support.microsoft.com/ru-ru/excel/functions/concat-function | https://easy-excel.com/excel-in-other-languages/excel-functions-in-russian/
        ("CONCAT", "СЦЕП"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://easy-excel.com/excel-in-other-languages/excel-functions-in-russian/ | https://excel-academy.ru/tools/excel-functions/
        ("CONCATENATE", "СЦЕПИТЬ"),
        ("CONFIDENCE", "ДОВЕРИТ"),
        ("CONFIDENCE.NORM", "ДОВЕРИТ.НОРМ"),
        ("CONFIDENCE.T", "ДОВЕРИТ.СТЬЮДЕНТ"),
        ("CONVERT", "ПРЕОБР"),
        ("CORREL", "КОРРЕЛ"),
        ("COUNT", "СЧЁТ"),
        ("COUNTA", "СЧЁТЗ"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://easy-excel.com/excel-in-other-languages/excel-functions-in-russian/ | https://excel-academy.ru/tools/excel-functions/
        ("COUNTBLANK", "СЧИТАТЬПУСТОТЫ"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://easy-excel.com/excel-in-other-languages/excel-functions-in-russian/ | https://excel-academy.ru/tools/excel-functions/
        ("COUNTIF", "СЧЁТЕСЛИ"),
        ("COUNTIFS", "СЧЁТЕСЛИМН"),
        ("COUPDAYBS", "ДНЕЙКУПОНДО"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("COUPDAYS", "ДНЕЙКУПОН"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("COUPDAYSNC", "ДНЕЙКУПОНПОСЛЕ"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("COUPNCD", "ДАТАКУПОНПОСЛЕ"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("COUPNUM", "ЧИСЛКУПОН"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("COUPPCD", "ДАТАКУПОНДО"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("COVAR", "КОВАР"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("COVARIANCE.P", "КОВАРИАЦИЯ.Г"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("COVARIANCE.S", "КОВАРИАЦИЯ.В"),
        ("CRITBINOM", "КРИТБИНОМ"),
        ("CUBEKPIMEMBER", "КУБЭЛЕМЕНТКИП"),
        ("CUBEMEMBER", "КУБЭЛЕМЕНТ"),
        // reviewed: Microsoft title, metadata and examples agree with Functiontranslator; syntax incorrectly says КУБПОРЭЛЕМЕНТ (CUBERANKEDMEMBER) | sources: https://support.microsoft.com/ru-ru/excel/functions/cubememberproperty-function | https://functiontranslator.com/ru/
        ("CUBEMEMBERPROPERTY", "КУБСВОЙСТВОЭЛЕМЕНТА"),
        ("CUBERANKEDMEMBER", "КУБПОРЭЛЕМЕНТ"),
        ("CUBESET", "КУБМНОЖ"),
        ("CUBESETCOUNT", "КУБЧИСЛОЭЛМНОЖ"),
        ("CUBEVALUE", "КУБЗНАЧЕНИЕ"),
        ("CUMIPMT", "ОБЩПЛАТ"),
        ("CUMPRINC", "ОБЩДОХОД"),
        ("DATE", "ДАТА"),
        ("DATEDIF", "РАЗНДАТ"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://easy-excel.com/excel-in-other-languages/excel-functions-in-russian/ | https://excel-academy.ru/tools/excel-functions/
        ("DATEVALUE", "ДАТАЗНАЧ"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("DAVERAGE", "ДСРЗНАЧ"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://easy-excel.com/excel-in-other-languages/excel-functions-in-russian/ | https://excel-academy.ru/tools/excel-functions/
        ("DAY", "ДЕНЬ"),
        // reviewed | sources: https://functiontranslator.com/ru/ | https://easy-excel.com/excel-in-other-languages/excel-functions-in-russian/ | https://excel-academy.ru/tools/excel-functions/
        ("DAYS", "ДНИ"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("DAYS360", "ДНЕЙ360"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("DB", "ФУО"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("DCOUNT", "БСЧЁТ"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("DCOUNTA", "БСЧЁТА"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("DDB", "ДДОБ"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://easy-excel.com/excel-in-other-languages/excel-functions-in-russian/ | https://excel-academy.ru/tools/excel-functions/
        ("DEC2BIN", "ДЕС.В.ДВ"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://easy-excel.com/excel-in-other-languages/excel-functions-in-russian/ | https://excel-academy.ru/tools/excel-functions/
        ("DEC2HEX", "ДЕС.В.ШЕСТН"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("DEC2OCT", "ДЕС.В.ВОСЬМ"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("DECIMAL", "ДЕС"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("DEGREES", "ГРАДУСЫ"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("DELTA", "ДЕЛЬТА"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("DEVSQ", "КВАДРОТКЛ"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("DGET", "БИЗВЛЕЧЬ"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("DISC", "СКИДКА"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("DMAX", "ДМАКС"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("DMIN", "ДМИН"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("DOLLAR", "РУБЛЬ"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("DOLLARDE", "РУБЛЬ.ДЕС"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("DOLLARFR", "РУБЛЬ.ДРОБЬ"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("DPRODUCT", "БДПРОИЗВЕД"),
        // reviewed: Microsoft title and meta give the Russian name; the syntax and examples were left untranslated (English name) | sources: https://support.microsoft.com/ru-ru/excel/functions/drop-function
        ("DROP", "СБРОСИТЬ"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("DSTDEV", "ДСТАНДОТКЛ"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("DSTDEVP", "ДСТАНДОТКЛП"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("DSUM", "БДСУММ"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("DURATION", "ДЛИТ"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("DVAR", "БДДИСП"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("DVARP", "БДДИСПП"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("EDATE", "ДАТАМЕС"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("EFFECT", "ЭФФЕКТ"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("ENCODEURL", "КОДИР.URL"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://easy-excel.com/excel-in-other-languages/excel-functions-in-russian/ | https://excel-academy.ru/tools/excel-functions/
        ("EOMONTH", "КОНМЕСЯЦА"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/
        ("ERF", "ФОШ"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/
        ("ERF.PRECISE", "ФОШ.ТОЧН"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/
        ("ERFC", "ДФОШ"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/
        ("ERFC.PRECISE", "ДФОШ.ТОЧН"),
        ("ERROR.TYPE", "ТИП.ОШИБКИ"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://easy-excel.com/excel-in-other-languages/excel-functions-in-russian/ | https://excel-academy.ru/tools/excel-functions/
        ("EVEN", "ЧЁТН"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://easy-excel.com/excel-in-other-languages/excel-functions-in-russian/ | https://excel-academy.ru/tools/excel-functions/
        ("EXACT", "СОВПАД"),
        ("EXPAND", "РАЗВЕРНУТЬ"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("EXPON.DIST", "ЭКСП.РАСП"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("EXPONDIST", "ЭКСПРАСП"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/
        ("F.DIST", "F.РАСП"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/
        ("F.DIST.RT", "F.РАСП.ПХ"),
        ("F.INV", "F.ОБР"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/
        ("F.INV.RT", "F.ОБР.ПХ"),
        ("F.TEST", "F.ТЕСТ"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://easy-excel.com/excel-in-other-languages/excel-functions-in-russian/ | https://excel-academy.ru/tools/excel-functions/
        ("FACT", "ФАКТР"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("FACTDOUBLE", "ДВФАКТР"),
        ("FALSE", "ЛОЖЬ"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/
        ("FDIST", "FРАСП"),
        // reviewed: Microsoft calc-error page and independent dynamic-array article show literal ФИЛЬТР formulas | sources: https://support.microsoft.com/ru-ru/excel/how-to-correct-a-calc-error | https://www.planetaexcel.ru/techniques/25/9457/
        ("FILTER", "ФИЛЬТР"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://easy-excel.com/excel-in-other-languages/excel-functions-in-russian/ | https://excel-academy.ru/tools/excel-functions/
        ("FILTERXML", "ФИЛЬТР.XML"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://easy-excel.com/excel-in-other-languages/excel-functions-in-russian/ | https://excel-academy.ru/tools/excel-functions/
        ("FIND", "НАЙТИ"),
        // reviewed: Two independent function tables agree (the Microsoft page could not be fetched) | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/
        ("FINDB", "НАЙТИБ"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/
        ("FINV", "FРАСПОБР"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/
        ("FISHER", "ФИШЕР"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/
        ("FISHERINV", "ФИШЕРОБР"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("FIXED", "ФИКСИРОВАННЫЙ"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://easy-excel.com/excel-in-other-languages/excel-functions-in-russian/ | https://excel-academy.ru/tools/excel-functions/
        ("FLOOR", "ОКРВНИЗ"),
        // reviewed | sources: https://functiontranslator.com/ru/ | https://easy-excel.com/excel-in-other-languages/excel-functions-in-russian/ | https://excel-academy.ru/tools/excel-functions/
        ("FLOOR.MATH", "ОКРВНИЗ.МАТ"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("FLOOR.PRECISE", "ОКРВНИЗ.ТОЧН"),
        ("FORECAST", "ПРЕДСКАЗ"),
        // reviewed: MS page syntax ПРЕДСКАЗ.ETS (Latin ETS); excel-translator statistical table agrees | sources: https://support.microsoft.com/ru-ru/excel/functions/forecast-ets-function | https://en.excel-translator.de/functions/statistical/
        ("FORECAST.ETS", "ПРЕДСКАЗ.ETS"),
        // reviewed: MS page title and syntax, excel-translator and a planetaexcel reader formula all spell it with a Cyrillic Е before a Latin TS | sources: https://support.microsoft.com/ru-ru/excel/functions/forecast-ets-confint-function | https://en.excel-translator.de/functions/statistical/ | https://www.planetaexcel.ru/techniques/25/2823/
        ("FORECAST.ETS.CONFINT", "ПРЕДСКАЗ.ЕTS.ДОВИНТЕРВАЛ"),
        // reviewed: MS page syntax ПРЕДСКАЗ.ETS.СЕЗОННОСТЬ; excel-translator agrees | sources: https://support.microsoft.com/ru-ru/excel/functions/forecast-ets-seasonality-function | https://en.excel-translator.de/functions/statistical/
        ("FORECAST.ETS.SEASONALITY", "ПРЕДСКАЗ.ETS.СЕЗОННОСТЬ"),
        // reviewed: MS page syntax ПРЕДСКАЗ.ETS.СТАТ; excel-translator agrees | sources: https://support.microsoft.com/ru-ru/excel/functions/forecast-ets-stat-function | https://en.excel-translator.de/functions/statistical/
        ("FORECAST.ETS.STAT", "ПРЕДСКАЗ.ETS.СТАТ"),
        // reviewed | sources: https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("FORECAST.LINEAR", "ПРЕДСКАЗ.ЛИНЕЙН"),
        // reviewed | sources: https://functiontranslator.com/ru/ | https://easy-excel.com/excel-in-other-languages/excel-functions-in-russian/ | https://excel-academy.ru/tools/excel-functions/
        ("FORMULATEXT", "Ф.ТЕКСТ"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://easy-excel.com/excel-in-other-languages/excel-functions-in-russian/ | https://excel-academy.ru/tools/excel-functions/
        ("FREQUENCY", "ЧАСТОТА"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/
        ("FTEST", "ФТЕСТ"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("FV", "БС"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("FVSCHEDULE", "БЗРАСПИС"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("GAMMA", "ГАММА"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("GAMMA.DIST", "ГАММА.РАСП"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("GAMMA.INV", "ГАММА.ОБР"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("GAMMADIST", "ГАММАРАСП"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("GAMMAINV", "ГАММАОБР"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("GAMMALN", "ГАММАНЛОГ"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("GAMMALN.PRECISE", "ГАММАНЛОГ.ТОЧН"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/
        ("GAUSS", "ГАУСС"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("GCD", "НОД"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("GEOMEAN", "СРГЕОМ"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/
        ("GESTEP", "ПОРОГ"),
        ("GROUPBY", "ГРУПППО"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("GROWTH", "РОСТ"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("HARMEAN", "СРГАРМ"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://easy-excel.com/excel-in-other-languages/excel-functions-in-russian/ | https://excel-academy.ru/tools/excel-functions/
        ("HEX2BIN", "ШЕСТН.В.ДВ"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://easy-excel.com/excel-in-other-languages/excel-functions-in-russian/ | https://excel-academy.ru/tools/excel-functions/
        ("HEX2DEC", "ШЕСТН.В.ДЕС"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("HEX2OCT", "ШЕСТН.В.ВОСЬМ"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://easy-excel.com/excel-in-other-languages/excel-functions-in-russian/ | https://excel-academy.ru/tools/excel-functions/
        ("HLOOKUP", "ГПР"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("HOUR", "ЧАС"),
        ("HSTACK", "ГСТОЛБИК"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://easy-excel.com/excel-in-other-languages/excel-functions-in-russian/ | https://excel-academy.ru/tools/excel-functions/
        ("HYPERLINK", "ГИПЕРССЫЛКА"),
        ("HYPGEOM.DIST", "ГИПЕРГЕОМ.РАСП"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/
        ("HYPGEOMDIST", "ГИПЕРГЕОМЕТ"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://easy-excel.com/excel-in-other-languages/excel-functions-in-russian/ | https://excel-academy.ru/tools/excel-functions/
        ("IF", "ЕСЛИ"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://easy-excel.com/excel-in-other-languages/excel-functions-in-russian/ | https://excel-academy.ru/tools/excel-functions/
        ("IFERROR", "ЕСЛИОШИБКА"),
        // reviewed: Microsoft title, meta and syntax agree, with Excel Academy and the excelworld title; two dictionaries and the excelworld syntax line spell ЕСЛИНД (kept as an alias) | sources: https://support.microsoft.com/ru-ru/excel/functions/ifna-function | https://excel-academy.ru/tools/excel-functions/ | http://www.excelworld.ru/publ/funcs/boolean/ifna/11-1-0-118
        ("IFNA", "ЕСНД"),
        // reviewed: Microsoft meta and examples, a worked formula on mister-office and a function table agree; the page title УСЛОВИЯ is a machine translation (kept as an alias) | sources: https://support.microsoft.com/ru-ru/excel/functions/ifs-function (meta, examples) | https://mister-office.com/funktsii-excel/ifs-eslimn.html (=E8*ЕСЛИМН(...)) | https://functiontranslator.com/ru/
        ("IFS", "ЕСЛИМН"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("IMABS", "МНИМ.ABS"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("IMAGINARY", "МНИМ.ЧАСТЬ"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("IMARGUMENT", "МНИМ.АРГУМЕНТ"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("IMCONJUGATE", "МНИМ.СОПРЯЖ"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("IMCOS", "МНИМ.COS"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("IMCOSH", "МНИМ.COSH"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("IMCOT", "МНИМ.COT"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("IMCSC", "МНИМ.CSC"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("IMCSCH", "МНИМ.CSCH"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("IMDIV", "МНИМ.ДЕЛ"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("IMEXP", "МНИМ.EXP"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("IMLN", "МНИМ.LN"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("IMLOG10", "МНИМ.LOG10"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("IMLOG2", "МНИМ.LOG2"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("IMPOWER", "МНИМ.СТЕПЕНЬ"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("IMPRODUCT", "МНИМ.ПРОИЗВЕД"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("IMREAL", "МНИМ.ВЕЩ"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("IMSEC", "МНИМ.SEC"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("IMSECH", "МНИМ.SECH"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("IMSIN", "МНИМ.SIN"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("IMSINH", "МНИМ.SINH"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("IMSQRT", "МНИМ.КОРЕНЬ"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("IMSUB", "МНИМ.РАЗН"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("IMSUM", "МНИМ.СУММ"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("IMTAN", "МНИМ.TAN"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://easy-excel.com/excel-in-other-languages/excel-functions-in-russian/ | https://excel-academy.ru/tools/excel-functions/
        ("INDEX", "ИНДЕКС"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://easy-excel.com/excel-in-other-languages/excel-functions-in-russian/ | https://excel-academy.ru/tools/excel-functions/
        ("INDIRECT", "ДВССЫЛ"),
        ("INFO", "ИНФОРМ"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://easy-excel.com/excel-in-other-languages/excel-functions-in-russian/ | https://excel-academy.ru/tools/excel-functions/
        ("INT", "ЦЕЛОЕ"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("INTERCEPT", "ОТРЕЗОК"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("INTRATE", "ИНОРМА"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("IPMT", "ПРПЛТ"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("IRR", "ВСД"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://easy-excel.com/excel-in-other-languages/excel-functions-in-russian/ | https://excel-academy.ru/tools/excel-functions/
        ("ISBLANK", "ЕПУСТО"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("ISERR", "ЕОШ"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://easy-excel.com/excel-in-other-languages/excel-functions-in-russian/ | https://excel-academy.ru/tools/excel-functions/
        ("ISERROR", "ЕОШИБКА"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://easy-excel.com/excel-in-other-languages/excel-functions-in-russian/ | https://excel-academy.ru/tools/excel-functions/
        ("ISEVEN", "ЕЧЁТН"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("ISFORMULA", "ЕФОРМУЛА"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://easy-excel.com/excel-in-other-languages/excel-functions-in-russian/ | https://excel-academy.ru/tools/excel-functions/
        ("ISLOGICAL", "ЕЛОГИЧ"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://easy-excel.com/excel-in-other-languages/excel-functions-in-russian/ | https://excel-academy.ru/tools/excel-functions/
        ("ISNA", "ЕНД"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://easy-excel.com/excel-in-other-languages/excel-functions-in-russian/ | https://excel-academy.ru/tools/excel-functions/
        ("ISNONTEXT", "ЕНЕТЕКСТ"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://easy-excel.com/excel-in-other-languages/excel-functions-in-russian/ | https://excel-academy.ru/tools/excel-functions/
        ("ISNUMBER", "ЕЧИСЛО"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/
        ("ISO.CEILING", "ISO.ОКРВВЕРХ"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://easy-excel.com/excel-in-other-languages/excel-functions-in-russian/ | https://excel-academy.ru/tools/excel-functions/
        ("ISODD", "ЕНЕЧЁТ"),
        // reviewed | sources: https://functiontranslator.com/ru/ | https://easy-excel.com/excel-in-other-languages/excel-functions-in-russian/ | https://excel-academy.ru/tools/excel-functions/
        ("ISOWEEKNUM", "НОМНЕДЕЛИ.ISO"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("ISPMT", "ПРОЦПЛАТ"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("ISREF", "ЕССЫЛКА"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://easy-excel.com/excel-in-other-languages/excel-functions-in-russian/ | https://excel-academy.ru/tools/excel-functions/
        ("ISTEXT", "ЕТЕКСТ"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/
        ("KURT", "ЭКСЦЕСС"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://easy-excel.com/excel-in-other-languages/excel-functions-in-russian/ | https://excel-academy.ru/tools/excel-functions/
        ("LARGE", "НАИБОЛЬШИЙ"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("LCM", "НОК"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://easy-excel.com/excel-in-other-languages/excel-functions-in-russian/ | https://excel-academy.ru/tools/excel-functions/
        ("LEFT", "ЛЕВСИМВ"),
        // reviewed: Microsoft page names it ("Функция ЛЕВБ является нерекомендуемой"); two function tables agree | sources: https://support.microsoft.com/ru-ru/excel/functions/left-function | https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/
        ("LEFTB", "ЛЕВБ"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://easy-excel.com/excel-in-other-languages/excel-functions-in-russian/ | https://excel-academy.ru/tools/excel-functions/
        ("LEN", "ДЛСТР"),
        // reviewed: Microsoft page names it ("Функция ДЛИНБ является нерекомендуемой"); two function tables agree | sources: https://support.microsoft.com/ru-ru/excel/functions/len-function | https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/
        ("LENB", "ДЛИНБ"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("LINEST", "ЛИНЕЙН"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/
        ("LOGEST", "ЛГРФПРИБЛ"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/
        ("LOGINV", "ЛОГНОРМОБР"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/
        ("LOGNORM.DIST", "ЛОГНОРМ.РАСП"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/
        ("LOGNORM.INV", "ЛОГНОРМ.ОБР"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/
        ("LOGNORMDIST", "ЛОГНОРМРАСП"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://easy-excel.com/excel-in-other-languages/excel-functions-in-russian/ | https://excel-academy.ru/tools/excel-functions/
        ("LOOKUP", "ПРОСМОТР"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://easy-excel.com/excel-in-other-languages/excel-functions-in-russian/ | https://excel-academy.ru/tools/excel-functions/
        ("LOWER", "СТРОЧН"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://easy-excel.com/excel-in-other-languages/excel-functions-in-russian/ | https://excel-academy.ru/tools/excel-functions/
        ("MATCH", "ПОИСКПОЗ"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://easy-excel.com/excel-in-other-languages/excel-functions-in-russian/ | https://excel-academy.ru/tools/excel-functions/
        ("MAX", "МАКС"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("MAXA", "МАКСА"),
        // reviewed | sources: https://easy-excel.com/excel-in-other-languages/excel-functions-in-russian/ | https://excel-academy.ru/tools/excel-functions/
        ("MAXIFS", "МАКСЕСЛИ"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("MDETERM", "МОПРЕД"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("MDURATION", "МДЛИТ"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://easy-excel.com/excel-in-other-languages/excel-functions-in-russian/ | https://excel-academy.ru/tools/excel-functions/
        ("MEDIAN", "МЕДИАНА"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://easy-excel.com/excel-in-other-languages/excel-functions-in-russian/ | https://excel-academy.ru/tools/excel-functions/
        ("MID", "ПСТР"),
        // reviewed: Microsoft page names it ("Функция ПСТРБ является нерекомендуемой"); two function tables agree | sources: https://support.microsoft.com/ru-ru/excel/functions/mid-function | https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/
        ("MIDB", "ПСТРБ"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://easy-excel.com/excel-in-other-languages/excel-functions-in-russian/ | https://excel-academy.ru/tools/excel-functions/
        ("MIN", "МИН"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("MINA", "МИНА"),
        // reviewed | sources: https://easy-excel.com/excel-in-other-languages/excel-functions-in-russian/ | https://excel-academy.ru/tools/excel-functions/
        ("MINIFS", "МИНЕСЛИ"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("MINUTE", "МИНУТЫ"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("MINVERSE", "МОБР"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("MIRR", "МВСД"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("MMULT", "МУМНОЖ"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://easy-excel.com/excel-in-other-languages/excel-functions-in-russian/ | https://excel-academy.ru/tools/excel-functions/
        ("MOD", "ОСТАТ"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://easy-excel.com/excel-in-other-languages/excel-functions-in-russian/ | https://excel-academy.ru/tools/excel-functions/
        ("MODE", "МОДА"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("MODE.MULT", "МОДА.НСК"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("MODE.SNGL", "МОДА.ОДН"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://easy-excel.com/excel-in-other-languages/excel-functions-in-russian/ | https://excel-academy.ru/tools/excel-functions/
        ("MONTH", "МЕСЯЦ"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://easy-excel.com/excel-in-other-languages/excel-functions-in-russian/ | https://excel-academy.ru/tools/excel-functions/
        ("MROUND", "ОКРУГЛТ"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("MULTINOMIAL", "МУЛЬТИНОМ"),
        // reviewed | sources: https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("MUNIT", "МЕДИН"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://easy-excel.com/excel-in-other-languages/excel-functions-in-russian/ | https://excel-academy.ru/tools/excel-functions/
        ("N", "Ч"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://easy-excel.com/excel-in-other-languages/excel-functions-in-russian/ | https://excel-academy.ru/tools/excel-functions/
        ("NA", "НД"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/
        ("NEGBINOM.DIST", "ОТРБИНОМ.РАСП"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/
        ("NEGBINOMDIST", "ОТРБИНОМРАСП"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://easy-excel.com/excel-in-other-languages/excel-functions-in-russian/ | https://excel-academy.ru/tools/excel-functions/
        ("NETWORKDAYS", "ЧИСТРАБДНИ"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://easy-excel.com/excel-in-other-languages/excel-functions-in-russian/ | https://excel-academy.ru/tools/excel-functions/
        ("NETWORKDAYS.INTL", "ЧИСТРАБДНИ.МЕЖД"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("NOMINAL", "НОМИНАЛ"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("NORM.DIST", "НОРМ.РАСП"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("NORM.INV", "НОРМ.ОБР"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("NORM.S.DIST", "НОРМ.СТ.РАСП"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("NORM.S.INV", "НОРМ.СТ.ОБР"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/
        ("NORMDIST", "НОРМРАСП"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/
        ("NORMINV", "НОРМОБР"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("NORMSDIST", "НОРМСТРАСП"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("NORMSINV", "НОРМСТОБР"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://easy-excel.com/excel-in-other-languages/excel-functions-in-russian/ | https://excel-academy.ru/tools/excel-functions/
        ("NOT", "НЕ"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://easy-excel.com/excel-in-other-languages/excel-functions-in-russian/ | https://excel-academy.ru/tools/excel-functions/
        ("NOW", "ТДАТА"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("NPER", "КПЕР"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("NPV", "ЧПС"),
        // reviewed | sources: https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("NUMBERVALUE", "ЧЗНАЧ"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("OCT2BIN", "ВОСЬМ.В.ДВ"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("OCT2DEC", "ВОСЬМ.В.ДЕС"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("OCT2HEX", "ВОСЬМ.В.ШЕСТН"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://easy-excel.com/excel-in-other-languages/excel-functions-in-russian/ | https://excel-academy.ru/tools/excel-functions/
        ("ODD", "НЕЧЁТ"),
        // reviewed: perfectxl, excel-translator and excelworld agree; MS example =ЦЕНАПЕРВНЕРЕГ( | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://en.excel-translator.de/functions/financial/ | http://www.excelworld.ru/stuff/worksheet_function/list_worksheet_function/all_worksheet_function/16-1-0-3
        ("ODDFPRICE", "ЦЕНАПЕРВНЕРЕГ"),
        // reviewed: perfectxl, excel-translator and excelworld agree; MS example =ДОХОДПЕРВНЕРЕГ( | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://en.excel-translator.de/functions/financial/ | http://www.excelworld.ru/stuff/worksheet_function/list_worksheet_function/all_worksheet_function/16-1-0-3
        ("ODDFYIELD", "ДОХОДПЕРВНЕРЕГ"),
        // reviewed: perfectxl, excel-translator and excelworld agree; MS example =ЦЕНАПОСЛНЕРЕГ( | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://en.excel-translator.de/functions/financial/ | http://www.excelworld.ru/stuff/worksheet_function/list_worksheet_function/all_worksheet_function/16-1-0-3
        ("ODDLPRICE", "ЦЕНАПОСЛНЕРЕГ"),
        // reviewed: perfectxl, excel-translator and excelworld agree; MS example =ДОХОДПОСЛНЕРЕГ( | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://en.excel-translator.de/functions/financial/ | http://www.excelworld.ru/stuff/worksheet_function/list_worksheet_function/all_worksheet_function/16-1-0-3
        ("ODDLYIELD", "ДОХОДПОСЛНЕРЕГ"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://easy-excel.com/excel-in-other-languages/excel-functions-in-russian/ | https://excel-academy.ru/tools/excel-functions/
        ("OFFSET", "СМЕЩ"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://easy-excel.com/excel-in-other-languages/excel-functions-in-russian/ | https://excel-academy.ru/tools/excel-functions/
        ("OR", "ИЛИ"),
        // reviewed | sources: https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("PDURATION", "ПДЛИТ"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://easy-excel.com/excel-in-other-languages/excel-functions-in-russian/ | https://excel-academy.ru/tools/excel-functions/
        ("PERCENTILE", "ПЕРСЕНТИЛЬ"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://easy-excel.com/excel-in-other-languages/excel-functions-in-russian/ | https://excel-academy.ru/tools/excel-functions/
        ("PERCENTILE.EXC", "ПРОЦЕНТИЛЬ.ИСКЛ"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://easy-excel.com/excel-in-other-languages/excel-functions-in-russian/ | https://excel-academy.ru/tools/excel-functions/
        ("PERCENTILE.INC", "ПРОЦЕНТИЛЬ.ВКЛ"),
        // reviewed: MS page title and example =ПРОЦЕНТ(; shagabutdinov uses it as the СВОДПО aggregator | sources: https://support.microsoft.com/ru-ru/excel/functions/percentof-function | https://shagabutdinov.ru/tpost/zjc1ca6uy1-funktsii-grupppo-groupby-i-svodpo-pivotb
        ("PERCENTOF", "ПРОЦЕНТ"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://easy-excel.com/excel-in-other-languages/excel-functions-in-russian/ | https://excel-academy.ru/tools/excel-functions/
        ("PERCENTRANK", "ПРОЦЕНТРАНГ"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://easy-excel.com/excel-in-other-languages/excel-functions-in-russian/ | https://excel-academy.ru/tools/excel-functions/
        ("PERCENTRANK.EXC", "ПРОЦЕНТРАНГ.ИСКЛ"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://easy-excel.com/excel-in-other-languages/excel-functions-in-russian/ | https://excel-academy.ru/tools/excel-functions/
        ("PERCENTRANK.INC", "ПРОЦЕНТРАНГ.ВКЛ"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("PERMUT", "ПЕРЕСТ"),
        // reviewed | sources: https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("PERMUTATIONA", "ПЕРЕСТА"),
        // reviewed | sources: https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("PHI", "ФИ"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://easy-excel.com/excel-in-other-languages/excel-functions-in-russian/ | https://excel-academy.ru/tools/excel-functions/
        ("PI", "ПИ"),
        ("PIVOTBY", "СВОДПО"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://easy-excel.com/excel-in-other-languages/excel-functions-in-russian/ | https://excel-academy.ru/tools/excel-functions/
        ("PMT", "ПЛТ"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/
        ("POISSON", "ПУАССОН"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/
        ("POISSON.DIST", "ПУАССОН.РАСП"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("POWER", "СТЕПЕНЬ"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("PPMT", "ОСПЛТ"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("PRICE", "ЦЕНА"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("PRICEDISC", "ЦЕНАСКИДКА"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("PRICEMAT", "ЦЕНАПОГАШ"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("PROB", "ВЕРОЯТНОСТЬ"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("PRODUCT", "ПРОИЗВЕД"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://easy-excel.com/excel-in-other-languages/excel-functions-in-russian/ | https://excel-academy.ru/tools/excel-functions/
        ("PROPER", "ПРОПНАЧ"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("PV", "ПС"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://easy-excel.com/excel-in-other-languages/excel-functions-in-russian/ | https://excel-academy.ru/tools/excel-functions/
        ("QUARTILE", "КВАРТИЛЬ"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://easy-excel.com/excel-in-other-languages/excel-functions-in-russian/ | https://excel-academy.ru/tools/excel-functions/
        ("QUARTILE.EXC", "КВАРТИЛЬ.ИСКЛ"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://easy-excel.com/excel-in-other-languages/excel-functions-in-russian/ | https://excel-academy.ru/tools/excel-functions/
        ("QUARTILE.INC", "КВАРТИЛЬ.ВКЛ"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("QUOTIENT", "ЧАСТНОЕ"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("RADIANS", "РАДИАНЫ"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://easy-excel.com/excel-in-other-languages/excel-functions-in-russian/ | https://excel-academy.ru/tools/excel-functions/
        ("RAND", "СЛЧИС"),
        ("RANDARRAY", "СЛУЧМАССИВ"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://easy-excel.com/excel-in-other-languages/excel-functions-in-russian/ | https://excel-academy.ru/tools/excel-functions/
        ("RANDBETWEEN", "СЛУЧМЕЖДУ"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://easy-excel.com/excel-in-other-languages/excel-functions-in-russian/ | https://excel-academy.ru/tools/excel-functions/
        ("RANK", "РАНГ"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://easy-excel.com/excel-in-other-languages/excel-functions-in-russian/ | https://excel-academy.ru/tools/excel-functions/
        ("RANK.AVG", "РАНГ.СР"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://easy-excel.com/excel-in-other-languages/excel-functions-in-russian/ | https://excel-academy.ru/tools/excel-functions/
        ("RANK.EQ", "РАНГ.РВ"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("RATE", "СТАВКА"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("RECEIVED", "ПОЛУЧЕНО"),
        ("REGEXEXTRACT", "РЕГИЗВЛЕЧЬ"),
        ("REGEXREPLACE", "РЕГЗАМЕНИТЬ"),
        ("REGEXTEST", "РЕГТЕСТ"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://easy-excel.com/excel-in-other-languages/excel-functions-in-russian/ | https://excel-academy.ru/tools/excel-functions/
        ("REPLACE", "ЗАМЕНИТЬ"),
        // reviewed: Microsoft page names it ("Функция ЗАМЕНИТЬБ является нерекомендуемой"); two function tables agree | sources: https://support.microsoft.com/ru-ru/excel/functions/replace-function | https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/
        ("REPLACEB", "ЗАМЕНИТЬБ"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://easy-excel.com/excel-in-other-languages/excel-functions-in-russian/ | https://excel-academy.ru/tools/excel-functions/
        ("REPT", "ПОВТОР"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://easy-excel.com/excel-in-other-languages/excel-functions-in-russian/ | https://excel-academy.ru/tools/excel-functions/
        ("RIGHT", "ПРАВСИМВ"),
        // reviewed: Microsoft page names it ("Функция ПРАВБ является нерекомендуемой"); two function tables agree | sources: https://support.microsoft.com/ru-ru/excel/functions/right-function | https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/
        ("RIGHTB", "ПРАВБ"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://easy-excel.com/excel-in-other-languages/excel-functions-in-russian/ | https://excel-academy.ru/tools/excel-functions/
        ("ROMAN", "РИМСКОЕ"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://easy-excel.com/excel-in-other-languages/excel-functions-in-russian/ | https://excel-academy.ru/tools/excel-functions/
        ("ROUND", "ОКРУГЛ"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://easy-excel.com/excel-in-other-languages/excel-functions-in-russian/ | https://excel-academy.ru/tools/excel-functions/
        ("ROUNDDOWN", "ОКРУГЛВНИЗ"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://easy-excel.com/excel-in-other-languages/excel-functions-in-russian/ | https://excel-academy.ru/tools/excel-functions/
        ("ROUNDUP", "ОКРУГЛВВЕРХ"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://easy-excel.com/excel-in-other-languages/excel-functions-in-russian/ | https://excel-academy.ru/tools/excel-functions/
        ("ROW", "СТРОКА"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://easy-excel.com/excel-in-other-languages/excel-functions-in-russian/ | https://excel-academy.ru/tools/excel-functions/
        ("ROWS", "ЧСТРОК"),
        // reviewed | sources: https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("RRI", "ЭКВ.СТАВКА"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/
        ("RSQ", "КВПИРСОН"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://easy-excel.com/excel-in-other-languages/excel-functions-in-russian/ | https://excel-academy.ru/tools/excel-functions/
        ("SEARCH", "ПОИСК"),
        // reviewed: Microsoft page names it ("Функция ПОИСКБ является нерекомендуемой"); two function tables agree | sources: https://support.microsoft.com/ru-ru/excel/functions/search-function | https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/
        ("SEARCHB", "ПОИСКБ"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("SECOND", "СЕКУНДЫ"),
        ("SEQUENCE", "ПОСЛЕДОВ"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/
        ("SERIESSUM", "РЯД.СУММ"),
        // reviewed | sources: https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("SHEET", "ЛИСТ"),
        // reviewed | sources: https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("SHEETS", "ЛИСТЫ"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("SIGN", "ЗНАК"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("SKEW", "СКОС"),
        // reviewed | sources: https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("SKEW.P", "СКОС.Г"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("SLN", "АПЛ"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("SLOPE", "НАКЛОН"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://easy-excel.com/excel-in-other-languages/excel-functions-in-russian/ | https://excel-academy.ru/tools/excel-functions/
        ("SMALL", "НАИМЕНЬШИЙ"),
        // reviewed: Independent article explicitly pairs English and Russian names and shows function syntax | sources: https://www.planetaexcel.ru/techniques/25/9457/
        ("SORT", "СОРТ"),
        ("SORTBY", "СОРТПО"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("SQRT", "КОРЕНЬ"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("SQRTPI", "КОРЕНЬПИ"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("STANDARDIZE", "НОРМАЛИЗАЦИЯ"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://easy-excel.com/excel-in-other-languages/excel-functions-in-russian/ | https://excel-academy.ru/tools/excel-functions/
        ("STDEV", "СТАНДОТКЛОН"),
        // reviewed | sources: https://functiontranslator.com/ru/ | https://easy-excel.com/excel-in-other-languages/excel-functions-in-russian/ | https://excel-academy.ru/tools/excel-functions/
        ("STDEV.P", "СТАНДОТКЛОН.Г"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://easy-excel.com/excel-in-other-languages/excel-functions-in-russian/ | https://excel-academy.ru/tools/excel-functions/
        ("STDEV.S", "СТАНДОТКЛОН.В"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("STDEVA", "СТАНДОТКЛОНА"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("STDEVP", "СТАНДОТКЛОНП"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("STDEVPA", "СТАНДОТКЛОНПА"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/
        ("STEYX", "СТОШYX"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://easy-excel.com/excel-in-other-languages/excel-functions-in-russian/ | https://excel-academy.ru/tools/excel-functions/
        ("SUBSTITUTE", "ПОДСТАВИТЬ"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://easy-excel.com/excel-in-other-languages/excel-functions-in-russian/ | https://excel-academy.ru/tools/excel-functions/
        ("SUBTOTAL", "ПРОМЕЖУТОЧНЫЕ.ИТОГИ"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://easy-excel.com/excel-in-other-languages/excel-functions-in-russian/ | https://excel-academy.ru/tools/excel-functions/
        ("SUM", "СУММ"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://easy-excel.com/excel-in-other-languages/excel-functions-in-russian/ | https://excel-academy.ru/tools/excel-functions/
        ("SUMIF", "СУММЕСЛИ"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://easy-excel.com/excel-in-other-languages/excel-functions-in-russian/ | https://excel-academy.ru/tools/excel-functions/
        ("SUMIFS", "СУММЕСЛИМН"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://easy-excel.com/excel-in-other-languages/excel-functions-in-russian/ | https://excel-academy.ru/tools/excel-functions/
        ("SUMPRODUCT", "СУММПРОИЗВ"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("SUMSQ", "СУММКВ"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("SUMX2MY2", "СУММРАЗНКВ"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("SUMX2PY2", "СУММСУММКВ"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("SUMXMY2", "СУММКВРАЗН"),
        // reviewed | sources: https://functiontranslator.com/ru/ | https://easy-excel.com/excel-in-other-languages/excel-functions-in-russian/ | https://excel-academy.ru/tools/excel-functions/
        ("SWITCH", "ПЕРЕКЛЮЧ"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("SYD", "АСЧ"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("T", "Т"),
        // reviewed: Independent detailed syntax and Microsoft syntax distinguish T.DIST from obsolete TDIST; both broad translation tables incorrectly conflate the pair | sources: https://www.formulafind.com/ru/функции/статистические/student-rasp-function | https://support.microsoft.com/ru-ru/excel/functions/t-dist-function
        ("T.DIST", "СТЬЮДЕНТ.РАСП"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/
        ("T.DIST.2T", "СТЬЮДЕНТ.РАСП.2Х"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/
        ("T.DIST.RT", "СТЬЮДЕНТ.РАСП.ПХ"),
        ("T.INV", "СТЬЮДЕНТ.ОБР"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/
        ("T.INV.2T", "СТЬЮДЕНТ.ОБР.2Х"),
        ("T.TEST", "СТЬЮДЕНТ.ТЕСТ"),
        // reviewed: Microsoft title and meta give the Russian name; the syntax and examples were left untranslated (English name) | sources: https://support.microsoft.com/ru-ru/excel/functions/take-function
        ("TAKE", "ВЗЯТЬ"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("TBILLEQ", "РАВНОКЧЕК"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("TBILLPRICE", "ЦЕНАКЧЕК"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("TBILLYIELD", "ДОХОДКЧЕК"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/
        ("TDIST", "СТЬЮДРАСП"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://easy-excel.com/excel-in-other-languages/excel-functions-in-russian/ | https://excel-academy.ru/tools/excel-functions/
        ("TEXT", "ТЕКСТ"),
        ("TEXTAFTER", "ТЕКСТПОСЛЕ"),
        ("TEXTBEFORE", "ТЕКСТДО"),
        // reviewed | sources: https://easy-excel.com/excel-in-other-languages/excel-functions-in-russian/ | https://excel-academy.ru/tools/excel-functions/
        ("TEXTJOIN", "ОБЪЕДИНИТЬ"),
        // reviewed: Microsoft syntax and examples give the Russian name; the title and meta were left untranslated (English name) | sources: https://support.microsoft.com/ru-ru/excel/functions/textsplit-function
        ("TEXTSPLIT", "ТЕКСТРАЗД"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("TIME", "ВРЕМЯ"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("TIMEVALUE", "ВРЕМЗНАЧ"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/
        ("TINV", "СТЬЮДРАСПОБР"),
        ("TOCOL", "ПОСТОЛБЦ"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://easy-excel.com/excel-in-other-languages/excel-functions-in-russian/ | https://excel-academy.ru/tools/excel-functions/
        ("TODAY", "СЕГОДНЯ"),
        ("TOROW", "ПОСТРОК"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://easy-excel.com/excel-in-other-languages/excel-functions-in-russian/ | https://excel-academy.ru/tools/excel-functions/
        ("TRANSPOSE", "ТРАНСП"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/
        ("TREND", "ТЕНДЕНЦИЯ"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://easy-excel.com/excel-in-other-languages/excel-functions-in-russian/ | https://excel-academy.ru/tools/excel-functions/
        ("TRIM", "СЖПРОБЕЛЫ"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("TRIMMEAN", "УРЕЗСРЕДНЕЕ"),
        // reviewed: MS page title and example =УРЕЗДИАПАЗОН(; shagabutdinov: "Новая функция УРЕЗДИАПАЗОН (TRIMRANGE)" | sources: https://support.microsoft.com/ru-ru/excel/functions/trimrange-function | https://shagabutdinov.ru/tpost/i65crd6tu1-novaya-funktsiya-urezdiapazon-trimrange
        ("TRIMRANGE", "УРЕЗДИАПАЗОН"),
        ("TRUE", "ИСТИНА"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("TRUNC", "ОТБР"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/
        ("TTEST", "ТТЕСТ"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("TYPE", "ТИП"),
        // reviewed | sources: https://functiontranslator.com/ru/ | https://easy-excel.com/excel-in-other-languages/excel-functions-in-russian/ | https://excel-academy.ru/tools/excel-functions/
        ("UNICHAR", "ЮНИСИМВ"),
        // reviewed: Independent article explicitly pairs English and Russian names and shows function syntax | sources: https://www.planetaexcel.ru/techniques/25/9457/
        ("UNIQUE", "УНИК"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://easy-excel.com/excel-in-other-languages/excel-functions-in-russian/ | https://excel-academy.ru/tools/excel-functions/
        ("UPPER", "ПРОПИСН"),
        // reviewed: excel-translator hidden-function table and excelworld list ДОЛЛСША | sources: https://en.excel-translator.de/functions/hidden/ | http://www.excelworld.ru/stuff/worksheet_function/list_worksheet_function/all_worksheet_function/16-1-0-3
        ("USDOLLAR", "ДОЛЛСША"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("VALUE", "ЗНАЧЕН"),
        // reviewed: Microsoft title, meta, syntax and examples all agree; the function tables only list the English name | sources: https://support.microsoft.com/ru-ru/excel/functions/valuetotext-function
        ("VALUETOTEXT", "ЗНАЧЕНИЕВТЕКСТ"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("VAR", "ДИСП"),
        // reviewed | sources: https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("VAR.P", "ДИСП.Г"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("VAR.S", "ДИСП.В"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("VARA", "ДИСПА"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("VARP", "ДИСПР"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("VARPA", "ДИСПРА"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("VDB", "ПУО"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://easy-excel.com/excel-in-other-languages/excel-functions-in-russian/ | https://excel-academy.ru/tools/excel-functions/
        ("VLOOKUP", "ВПР"),
        ("VSTACK", "ВСТОЛБИК"),
        // reviewed | sources: https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("WEBSERVICE", "ВЕБСЛУЖБА"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://easy-excel.com/excel-in-other-languages/excel-functions-in-russian/ | https://excel-academy.ru/tools/excel-functions/
        ("WEEKDAY", "ДЕНЬНЕД"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://easy-excel.com/excel-in-other-languages/excel-functions-in-russian/ | https://excel-academy.ru/tools/excel-functions/
        ("WEEKNUM", "НОМНЕДЕЛИ"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("WEIBULL", "ВЕЙБУЛЛ"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("WEIBULL.DIST", "ВЕЙБУЛЛ.РАСП"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("WORKDAY", "РАБДЕНЬ"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("WORKDAY.INTL", "РАБДЕНЬ.МЕЖД"),
        // reviewed: Microsoft syntax and examples give the Russian name; the title and meta were left untranslated (English name) | sources: https://support.microsoft.com/ru-ru/excel/functions/wrapcols-function
        ("WRAPCOLS", "СВЕРНСТОЛБЦ"),
        // reviewed: Microsoft syntax and examples give the Russian name; the title and meta were left untranslated (English name) | sources: https://support.microsoft.com/ru-ru/excel/functions/wraprows-function
        ("WRAPROWS", "СВЕРНСТРОК"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("XIRR", "ЧИСТВНДОХ"),
        ("XLOOKUP", "ПРОСМОТРX"),
        // reviewed: Microsoft title, meta, syntax and examples all agree (Latin X, like ПРОСМОТРX); the function tables only list the English name | sources: https://support.microsoft.com/ru-ru/excel/functions/xmatch-function
        ("XMATCH", "ПОИСКПОЗX"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("XNPV", "ЧИСТНЗ"),
        // reviewed | sources: https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("XOR", "ИСКЛИЛИ"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://easy-excel.com/excel-in-other-languages/excel-functions-in-russian/ | https://excel-academy.ru/tools/excel-functions/
        ("YEAR", "ГОД"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("YEARFRAC", "ДОЛЯГОДА"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("YIELD", "ДОХОД"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("YIELDDISC", "ДОХОДСКИДКА"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/ | https://excel-academy.ru/tools/excel-functions/
        ("YIELDMAT", "ДОХОДПОГАШ"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/
        ("Z.TEST", "Z.ТЕСТ"),
        // reviewed | sources: https://www.perfectxl.com/academy/functions/translations/english-russian/ | https://functiontranslator.com/ru/
        ("ZTEST", "ZТЕСТ"),
    ],
};
