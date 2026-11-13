import { readdirSync, readFileSync, statSync } from "node:fs";
import { join } from "node:path";
import ts from "typescript";
const root=process.argv[2];const out=[];
(function w(d){for(const f of readdirSync(d)){const p=join(d,f);if(statSync(p).isDirectory())w(p);else if(/\.tsx?$/.test(p)&&!/\.test\./.test(p)){const sf=ts.createSourceFile(p,readFileSync(p,"utf8"),99,true,p.endsWith("x")?4:3);let c=0;const l=[];
(function v(n){if((ts.isStringLiteral(n)||ts.isNoSubstitutionTemplateLiteral(n)||ts.isTemplateExpression(n))&&!ts.isImportDeclaration(n.parent)&&!ts.isJsxAttribute(n.parent)){const t=ts.isTemplateExpression(n)?n.head.text+n.templateSpans.map(s=>"{}"+s.literal.text).join(""):n.text;if(/\p{L}{3,}.*\s.*\p{L}{2,}/u.test(t)&&/[àèéìòù]|\b(il|la|di|che|non|per|con|una?|del|della|sono|è)\b/i.test(t)){c++;l.push(sf.getLineAndCharacterOfPosition(n.getStart()).line+1+": "+t.slice(0,70))}}ts.forEachChild(n,v)})(sf);if(c)out.push([p,c,l])}}})(root);
for(const [p,c,l] of out.sort((a,b)=>b[1]-a[1])){console.log(c,p);if(process.argv[3])console.log(l.map(x=>"   "+x).join("\n"))}
console.log("TOTAL",out.reduce((a,b)=>a+b[1],0));
