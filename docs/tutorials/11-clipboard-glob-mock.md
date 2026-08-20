# 11 · 剪贴板、目录通配、假数据

## 剪贴板

复制表格或 CSV 文本后：

```sql
LOAD t FROM 'clip:';
-- 或
LOAD t FROM read_clipboard();

SELECT * FROM t LIMIT 20;
```

```sql
LOAD t FROM 'clipboard:' WITH (delim='\t');
```

若剪贴板是文件路径且扩展名是 xlsx/csv，会按文件读。文本默认当 CSV 嗅探分隔符。

## 目录 / glob

同构文件按**列名**合并（多出来的列补上，缺的是 NULL）：

```sql
LOAD logs FROM './logs_*.csv';
SELECT _source, COUNT(*) FROM logs GROUP BY _source;
```

```sql
LOAD sales FROM EACH GLOB './sales_*.xlsx' WITH (
  format='excel',
  sheet='Sheet1',
  encoding='gbk'      -- 仅对其中的 csv/json 文本有意义；xlsx 忽略编码
);
```

`format='glob'` 也可。没有匹配到文件会报错。

混合 csv + xlsx 的 glob：按扩展名分发到对应解码器。

## 假数据（试 SQL、演示）

```sql
LOAD demo FROM mock_data(
  20,
  '用户名:name',
  '电话:phone',
  '邮箱:email',
  '公司:company',
  '城市:city'
);

SELECT * FROM demo LIMIT 8;
```

类型只有：`name` `phone` `email` `company` `city`。未知类型填 `N/A`。

```bash
sqlxls "SELECT * FROM mock_data(5, '用户名:name', '城:city')"
```

## 和真实表一起

```sql
LOAD real FROM 'users.csv';
LOAD fake FROM mock_data(3, '用户名:name');
SELECT * FROM real
UNION ALL
SELECT * FROM fake;
```

列名要对齐；`mock_data` 的列名就是你写的中文/英文标识。

## 下一步

[常见问题](12-faq.md)
