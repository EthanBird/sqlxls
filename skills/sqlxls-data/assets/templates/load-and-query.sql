-- 预览：改 locator / sheet 后执行
--   bash scripts/run.sh assets/templates/load-and-query.sql
LOAD t FROM 'data.xlsx' WITH (format='excel', sheet='Sheet1');

SELECT * FROM t LIMIT 20;
