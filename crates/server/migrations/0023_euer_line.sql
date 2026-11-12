-- the Zeile of the Anlage EÜR an account counts on; none for balance sheet accounts
ALTER TABLE ledger_account ADD COLUMN euer_line TEXT CHECK (euer_line IN (
    '12', '15', '16', '17', '18', '27', '29', '30', '33', '36', '39', '44', '55', '56', '57', '58',
    '60', '63'
));

UPDATE ledger_account SET euer_line = CASE (SELECT chart FROM books)
    WHEN 'skr04' THEN CASE number
        WHEN '4185' THEN '12' WHEN '4300' THEN '15' WHEN '4400' THEN '15'
        WHEN '4840' THEN '16' WHEN '7100' THEN '16'
        WHEN '3801' THEN '17' WHEN '3806' THEN '17' WHEN '3811' THEN '17' WHEN '3816' THEN '17'
        WHEN '1401' THEN '57' WHEN '1406' THEN '57' WHEN '3820' THEN '58' WHEN '3841' THEN '58'
        WHEN '5300' THEN '27' WHEN '5400' THEN '27' WHEN '5900' THEN '29' WHEN '6310' THEN '39'
        WHEN '6220' THEN '33' WHEN '6260' THEN '36' WHEN '7300' THEN '56' WHEN '6640' THEN '63'
    END
    ELSE CASE number
        WHEN '8195' THEN '12' WHEN '8300' THEN '15' WHEN '8400' THEN '15'
        WHEN '2660' THEN '16' WHEN '2650' THEN '16'
        WHEN '1771' THEN '17' WHEN '1776' THEN '17' WHEN '1761' THEN '17' WHEN '1766' THEN '17'
        WHEN '1571' THEN '57' WHEN '1576' THEN '57' WHEN '1780' THEN '58' WHEN '1790' THEN '58'
        WHEN '3300' THEN '27' WHEN '3400' THEN '27' WHEN '3100' THEN '29' WHEN '4210' THEN '39'
        WHEN '4830' THEN '33' WHEN '4855' THEN '36' WHEN '2100' THEN '56' WHEN '4650' THEN '63'
    END
END;

UPDATE ledger_account SET euer_line = CASE type WHEN 'income' THEN '15' WHEN 'expense' THEN '60' END
WHERE euer_line IS NULL AND type IN ('income', 'expense');
